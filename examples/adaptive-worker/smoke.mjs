import assert from "node:assert/strict";
import { readFile, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";
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
      signal: init.signal ?? lifecycle.signal,
    });
  const client = createFlaggoClient({
    contracts: service.connection.contracts,
    decisionServiceUrl: service.decisionUrl,
    credential: { mode: "local-development" },
    fetch: fetchWithAbort,
    random: () => 0.25,
  });
  const telemetry = new LocalOtelLogs(service.paths.telemetry);
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
        service.publication.contractDigest,
      );
      assert.equal(
        result.decision.executableDigest,
        service.publication.activeExecutableDigest,
      );
      assert.ok(result.operations.some((operation) =>
        operation.startsWith("batch-applied:")
      ));
    }

    await telemetry.flush();
    const telemetryFromDisk = (await readFile(service.paths.telemetry, "utf8"))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    assert.deepEqual(telemetryFromDisk, telemetry.events);
    assert.deepEqual(
      new Set(telemetry.events.map((event) => event.eventName)),
      new Set([
        "worker.item.enqueued",
        "worker.item.completed",
        "worker.queue.depth",
        "worker.queue.pressure",
        "worker.processing.latency",
        "worker.batch.applied",
      ]),
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
      contractDigest: service.publication.contractDigest,
      executableDigest: service.publication.activeExecutableDigest,
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
