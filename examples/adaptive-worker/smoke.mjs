import assert from "node:assert/strict";
import { readFile, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createDecisionClient,
} from "../../packages/sdk-typescript/dist/runtime/index.js";
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
import { startAdaptiveWorkerService } from "./service.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `adaptive-worker-smoke-${process.pid}-${Date.now()}`,
);

async function runSmoke(lifecycle) {
  const service = await startAdaptiveWorkerService({
    lifecycle,
    runDirectory,
    writeConnection: false,
  });
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal === undefined
        ? lifecycle.signal
        : AbortSignal.any([init.signal, lifecycle.signal]),
    });
  const telemetry = new LocalOtelLogs(service.paths.telemetry);
  const client = createDecisionClient({
    bindings: service.connection.bindings,
    baseUrl: service.decisionUrl,
    credential: { mode: "local-development" },
    fetch: fetchWithAbort,
    random: () => 0.25,
    telemetry: { logger: telemetry },
  });
  try {
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
    assert.equal(steady.appliedBatchSize, 3);
    assert.equal(steady.decision.evaluation.source, "default");
    assert.equal(burst.appliedBatchSize, 6);
    assert.deepEqual(
      burst.decision.evaluation,
      { source: "rule", rule: "high-pressure" },
    );
    assert.equal(slowDownstream.appliedBatchSize, 3);
    assert.equal(recovery.appliedBatchSize, 3);
    assert.equal(worker.queueDepth, 0);
    for (const result of [steady, burst, slowDownstream, recovery]) {
      assert.equal(
        result.decision.contractDigest,
        service.deployment.contractDigest,
      );
      assert.equal(
        result.decision.executableDigest,
        service.deployment.activeExecutableDigest,
      );
      assert.ok(result.operations.some((operation) =>
        operation.startsWith("batch-applied:")
      ));
    }

    await telemetry.flush();
    const telemetryFromDisk = JSON.parse(
      (await readFile(service.paths.telemetry, "utf8")).trim(),
    );
    assert.deepEqual(telemetryFromDisk, telemetry.otlpJson);
    assert.deepEqual(
      new Set(telemetry.events.map((event) => event.eventName)),
      new Set([
        "flaggo.decision.received",
        "flaggo.outcome.observed",
        "worker.item.enqueued",
        "worker.item.completed",
        "worker.queue.depth",
        "worker.queue.pressure",
        "worker.processing.latency",
        "worker.batch.applied",
      ]),
    );
    const decisionEvents = telemetry.events.filter((event) =>
      event.eventName === "flaggo.decision.received");
    const outcomeEvents = telemetry.events.filter((event) =>
      event.eventName === "flaggo.outcome.observed");
    assert.equal(decisionEvents.length, 4);
    assert.equal(outcomeEvents.length, 4);
    assert.equal(
      decisionEvents[0].attributes["flaggo.correlation.workerId"],
      "adaptive-worker-1",
    );
    assert.equal(
      outcomeEvents[0].attributes["flaggo.evidence.binding"],
      "demo.workerBatchSize.processingLatencyMs",
    );
    assert.equal(
      telemetryFromDisk.resourceLogs[0].scopeLogs[0].logRecords
        .filter((record) => record.eventName?.startsWith("flaggo.")).length,
      8,
    );

    await service.decision.stop();
    const unavailable = new AdaptiveWorker(
      client,
      telemetry,
      new DeterministicWorkerClock(),
    );
    await assert.rejects(() => unavailable.runTick("steady"));

    process.stdout.write(`${JSON.stringify({
      status: "passed",
      contractDigest: service.deployment.contractDigest,
      executableDigest: service.deployment.activeExecutableDigest,
      steadyBatchSize: steady.appliedBatchSize,
      burstBatchSize: burst.appliedBatchSize,
      recoveryBatchSize: recovery.appliedBatchSize,
      telemetryEvents: telemetry.events.length,
    }, null, 2)}\n`);
  } finally {
    await telemetry.shutdown();
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
