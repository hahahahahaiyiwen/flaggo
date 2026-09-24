import assert from "node:assert/strict";
import { readFile, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";
import { publishJsonGeneration } from "../tetris-integration/bootstrap.mjs";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
} from "../tetris-integration/host-process.mjs";
import {
  AdaptiveWorker,
  DeterministicWorkerClock,
} from "./dist/adaptive-worker.js";
import { LocalOtelLogs } from "./dist/telemetry.js";
import { catalog } from "./dist/generated/catalog.js";
import {
  createAdaptiveWorkerState,
  decisionKey,
} from "./scenario.mjs";
import { startAdaptiveWorkerService } from "./service.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `adaptive-worker-smoke-${process.pid}-${Date.now()}`,
);

async function runSmoke(lifecycle) {
  const logProviders = [];
  const createLogs = (path) => {
    const provider = new LocalOtelLogs(path);
    logProviders.push(provider);
    return provider;
  };
  try {
    const service = await startAdaptiveWorkerService({
      lifecycle,
      runDirectory,
      writeConnection: false,
    });
    const { bootstrap, data, dataUrl, paths } = service;
    assert.equal(bootstrap.approvalRequired, true);
    const fetchWithAbort = (input, init = {}) =>
      fetch(input, {
        ...init,
        signal: init.signal ?? lifecycle.signal,
      });
    const client = createFlaggoClient({
      catalog,
      receipt: bootstrap.receipt,
      dataPlaneUrl: dataUrl,
      dataPlaneCredential: { mode: "local-development" },
      availabilityFallback: { mode: "disabled", retries: 0 },
      fetch: fetchWithAbort,
    });
    const receipt = bootstrap.receipt;
    const accepted = receipt.acceptedDefinitions[decisionKey];
    assert.ok(accepted);
    assert.equal(receipt.bundleDigest, catalog.bundleDigest);
    assert.equal(
      accepted.contractDigest,
      catalog.decisions[decisionKey].contractDigest,
    );

    const telemetry = createLogs(paths.telemetry);
    const worker = new AdaptiveWorker(
      client,
      telemetry,
      new DeterministicWorkerClock(),
    );
    const [steady, burst, slowDownstream, recovery] =
      await worker.runProfiles([
        "steady",
        "burst",
        "slow-downstream",
        "recovery",
      ]);
    assert.ok(steady && burst && slowDownstream && recovery);
    assert.equal(steady.decision.source, "server");
    assert.equal(steady.appliedBatchSize, 3);
    assert.ok(steady.queuePressure < 0.7);
    assert.equal(burst.decision.source, "server");
    assert.equal(burst.appliedBatchSize, 6);
    assert.equal(burst.queuePressure, 0.7);
    assert.equal(recovery.appliedBatchSize, 3);
    assert.ok(recovery.queuePressure < 0.7);
    assert.equal(worker.queueDepth, 0);
    assert.equal(
      steady.decision.targetProvenance[0]?.claimedId,
      "worker-canary",
    );
    assert.equal(
      steady.decision.targetProvenance[0]?.resolvedId,
      "adaptive-workers",
    );
    assert.equal(
      steady.decision.targetProvenance[0]?.source,
      "server-replaced",
    );
    for (const result of [steady, burst, slowDownstream, recovery]) {
      assert.equal(result.decision.confidence, null);
      assert.equal(result.decision.policy.result, "approved");
      assert.ok(result.confirmation);
      assert.ok(
        result.operations.findIndex((operation) =>
          operation.startsWith("batch-applied:")
        ) <
          result.operations.findIndex((operation) =>
            operation.startsWith("exposure-confirmed:")
          ),
      );
    }

    assert.ok(burst.confirmation);
    assert.equal(burst.decision.source, "server");
    assert.equal(burst.decision.exposure.confirmationRequired, true);
    const repeatedConfirmation = await client.exposures.confirm(
      burst.decision.decisionId,
      burst.decision.exposure.confirmToken,
      { appliedAt: burst.appliedAt },
    );
    assert.equal(
      repeatedConfirmation.exposureId,
      burst.confirmation.exposureId,
    );

    await telemetry.flush();
    const telemetryFromDisk = (await readFile(paths.telemetry, "utf8"))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    assert.deepEqual(telemetryFromDisk, telemetry.events);
    const expectedEvents = new Set([
      "worker.item.enqueued",
      "worker.item.completed",
      "worker.queue.depth",
      "worker.queue.pressure",
      "worker.processing.latency",
      "worker.batch.applied",
    ]);
    assert.deepEqual(
      new Set(telemetry.events.map((event) => event.eventName)),
      expectedEvents,
    );
    for (const event of telemetry.events) {
      assert.ok(BigInt(event.timeUnixNano) > 0n);
    }
    const valuesFor = (key) => telemetry.events
      .filter((event) => event.eventName === key)
      .map((event) => event.body);
    assert.deepEqual(
      valuesFor("worker.queue.depth"),
      [2, 0, 8, 2, 5, 2, 3, 0],
    );
    assert.deepEqual(
      valuesFor("worker.queue.pressure"),
      [0.175, 0.7, 0.6174999999999999, 0.44249999999999995],
    );
    assert.deepEqual(
      valuesFor("worker.processing.latency"),
      [10, 20, 10, 20, 30, 40, 50, 60, 70, 80, 60, 100, 140, 90],
    );
    const enqueued = valuesFor("worker.item.enqueued");
    const completed = valuesFor("worker.item.completed");
    assert.deepEqual(
      telemetry.events.filter((record) => record.eventName === "worker.batch.applied")
        .map((record) => record.attributes["flaggo.exposure.id"]),
      [steady, burst, slowDownstream, recovery].map((result) => result.confirmation.exposureId),
    );
    assert.equal(enqueued.length, 14);
    assert.equal(completed.length, 14);
    assert.deepEqual(enqueued[0], {
      itemId: "steady-1-1",
      processingMs: 10,
      shouldFail: false,
    });
    assert.deepEqual(enqueued.at(-1), {
      itemId: "recovery-4-1",
      processingMs: 10,
      shouldFail: false,
    });
    assert.deepEqual(
      completed.filter(({ succeeded }) => !succeeded),
      [
        {
          itemId: "slow-downstream-3-3",
          succeeded: false,
        },
      ],
    );

    let loseConfirmationResponse = true;
    let recoveryDecisionRequests = 0;
    let recoveryConfirmationRequests = 0;
    const recoveryRequestOrder = [];
    const lossyFetch = async (input, init = {}) => {
      const url = String(input);
      if (url.includes("/v1/decisions/")) {
        recoveryDecisionRequests += 1;
        recoveryRequestOrder.push("decision");
      }
      if (url.includes("/v1/exposures/")) {
        recoveryConfirmationRequests += 1;
        recoveryRequestOrder.push("confirmation");
      }
      const response = await fetchWithAbort(input, init);
      if (url.includes("/v1/exposures/")) {
        if (loseConfirmationResponse) {
          loseConfirmationResponse = false;
          await response.arrayBuffer();
          throw new TypeError("Simulated lost confirmation response.");
        }
      }
      return response;
    };
    const confirmationRecoveryClient = createFlaggoClient({
      catalog,
      receipt,
      dataPlaneUrl: dataUrl,
      dataPlaneCredential: { mode: "local-development" },
      availabilityFallback: { mode: "disabled", retries: 0 },
      fetch: lossyFetch,
    });
    const confirmationRecoveryWorker = new AdaptiveWorker(
      confirmationRecoveryClient,
      createLogs(),
      new DeterministicWorkerClock(),
      8,
      100,
      "confirmation-recovery-worker",
    );
    await assert.rejects(
      () => confirmationRecoveryWorker.runTick("steady"),
      /lost confirmation response/iu,
    );
    assert.equal(confirmationRecoveryWorker.queueDepth, 0);
    assert.equal(confirmationRecoveryWorker.hasPendingConfirmation, true);
    const postRecoveryTick =
      await confirmationRecoveryWorker.runTick("recovery");
    assert.equal(postRecoveryTick.appliedBatchSize, 3);
    assert.equal(confirmationRecoveryWorker.queueDepth, 0);
    assert.equal(confirmationRecoveryWorker.hasPendingConfirmation, false);
    assert.equal(recoveryDecisionRequests, 2);
    assert.equal(recoveryConfirmationRequests, 3);
    assert.deepEqual(
      recoveryRequestOrder,
      [
        "decision",
        "confirmation",
        "confirmation",
        "decision",
        "confirmation",
      ],
    );

    const coolingState = createAdaptiveWorkerState(receipt, {
      lastChangedAt: new Date(),
    });
    await publishJsonGeneration(
      bootstrap.publication.rootPath,
      {
        receipt,
        state: coolingState,
        evidence: bootstrap.evidence,
      },
      lifecycle.signal,
    );
    const coolingWorker = new AdaptiveWorker(
      client,
      createLogs(),
      new DeterministicWorkerClock(),
      8,
      100,
      "cooldown-worker",
    );
    const policyFallback = await coolingWorker.runTick("burst");
    assert.equal(policyFallback.decision.source, "server");
    assert.equal(policyFallback.decision.decisionMode, "fallback");
    assert.equal(policyFallback.appliedBatchSize, 3);
    assert.equal(policyFallback.decision.fallback.source, "server");
    assert.ok(
      policyFallback.decision.policy.reasons.includes("cooldown_active"),
    );
    assert.equal(policyFallback.confirmation, undefined);

    const clientBase = {
      catalog,
      receipt,
      dataPlaneUrl: dataUrl,
      dataPlaneCredential: { mode: "local-development" },
      fetch: fetchWithAbort,
    };
    const fallbackDisabled = createFlaggoClient({
      ...clientBase,
      availabilityFallback: { mode: "disabled", retries: 0 },
    });
    const fallbackEnabled = createFlaggoClient({
      ...clientBase,
      availabilityFallback: { mode: "local-default", retries: 0 },
    });
    await data.stop();
    const unavailableWithoutFallback = new AdaptiveWorker(
      fallbackDisabled,
      createLogs(),
      new DeterministicWorkerClock(),
      8,
      100,
      "fallback-disabled-worker",
    );
    await assert.rejects(
      () => unavailableWithoutFallback.runTick("steady"),
    );
    const unavailableWithFallback = new AdaptiveWorker(
      fallbackEnabled,
      createLogs(),
      new DeterministicWorkerClock(),
      8,
      100,
      "fallback-enabled-worker",
    );
    const clientFallback = await unavailableWithFallback.runTick("steady");
    assert.equal(clientFallback.decision.source, "client-fallback");
    assert.equal(clientFallback.appliedBatchSize, 3);
    assert.equal(clientFallback.decision.fallback.source, "client-fallback");
    assert.equal(clientFallback.confirmation, undefined);
    assert.equal("decisionId" in clientFallback.decision, false);

    lifecycle.assertHealthy();
    process.stdout.write(`${JSON.stringify({
      status: "passed",
      bundleDigest: receipt.bundleDigest,
      steadyBatchSize: steady.appliedBatchSize,
      burstBatchSize: burst.appliedBatchSize,
      recoveryBatchSize: recovery.appliedBatchSize,
      serverFallbackBatchSize: policyFallback.appliedBatchSize,
      serverFallbackSource: policyFallback.decision.fallback.source,
      clientFallbackBatchSize: clientFallback.appliedBatchSize,
      clientFallbackSource: clientFallback.decision.fallback.source,
      repeatedExposureId: repeatedConfirmation.exposureId,
      telemetryEvents: telemetry.events.length,
    }, null, 2)}\n`);
  } finally {
    await Promise.all(logProviders.map((provider) => provider.shutdown()));
  }
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () =>
      rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => runSmoke(lifecycle),
      lifecycle,
    );
  } catch (error) {
    if (lifecycle.signalExitCode !== undefined) {
      process.exitCode = lifecycle.signalExitCode;
    } else {
      process.exitCode = 1;
      process.stderr.write(`${formatError(error)}\n`);
    }
  } finally {
    uninstallSignalHandlers();
  }
}

function formatError(error) {
  return error instanceof Error
    ? error.stack ?? error.message
    : String(error);
}

await main();
