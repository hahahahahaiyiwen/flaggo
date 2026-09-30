import assert from "node:assert/strict";
import { createServer } from "node:http";
import { rm } from "node:fs/promises";
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
import { AdaptiveWorkerTelemetry } from "./dist/telemetry.js";
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
  const proxy = await startRecordingProxy(service.otelIngestionUrl);
  lifecycle.trackHost(proxy);
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal === undefined
        ? lifecycle.signal
        : AbortSignal.any([init.signal, lifecycle.signal]),
    });
  const telemetry = new AdaptiveWorkerTelemetry({
    capture: true,
    flaggoOtlpBaseUrl: proxy.url,
  });
  const client = createDecisionClient({
    bindings: service.connection.bindings,
    baseUrl: service.decisionUrl,
    credential: { mode: "local-development" },
    fetch: fetchWithAbort,
    random: () => 0.25,
    telemetry: { logger: telemetry.flaggoLogger },
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
    assert.deepEqual(
      new Set(telemetry.events.map((event) => event.eventName)),
      new Set([
        "flaggo.decision.received",
        "flaggo.outcome.observed",
        "worker.item.enqueued",
        "worker.item.completed",
        "worker.batch.applied",
      ]),
    );
    const decisionEvents = telemetry.events.filter((event) =>
      event.eventName === "flaggo.decision.received");
    const outcomeEvents = telemetry.events.filter((event) =>
      event.eventName === "flaggo.outcome.observed");
    assert.equal(decisionEvents.length, 4);
    assert.equal(outcomeEvents.length, 4);
    assert.ok(decisionEvents.every((event) =>
      event.instrumentationScope.name === "@flaggo/sdk"
    ));
    assert.ok(outcomeEvents.every((event) =>
      event.instrumentationScope.name === "@flaggo/sdk"
    ));
    assert.ok(telemetry.events
      .filter((event) => event.eventName?.startsWith("worker.") === true)
      .every((event) =>
        event.instrumentationScope.name === "adaptive-worker.app"
      ));
    assert.equal(
      decisionEvents[0].attributes["flaggo.correlation.workerId"],
      "adaptive-worker-1",
    );
    assert.equal(
      outcomeEvents[0].attributes["flaggo.evidence.binding"],
      "demo.workerBatchSize.processingLatencyMs",
    );

    const workerSpans = telemetry.spans.filter((span) =>
      span.name === "worker.tick"
    );
    assert.equal(workerSpans.length, 4);
    const workerTraceIds = new Set(
      workerSpans.map((span) => span.spanContext().traceId),
    );
    assert.ok(decisionEvents.every((event) =>
      event.spanContext !== undefined
      && workerTraceIds.has(event.spanContext.traceId)
    ));
    assert.ok(outcomeEvents.every((event) =>
      event.spanContext !== undefined
      && workerTraceIds.has(event.spanContext.traceId)
    ));

    const metricNames = new Set(telemetry.metrics.flatMap((resourceMetrics) =>
      resourceMetrics.scopeMetrics.flatMap((scope) =>
        scope.metrics.map((metric) => metric.descriptor.name)
      )
    ));
    assert.deepEqual(metricNames, new Set([
      "worker.batch.size",
      "worker.processing.latency",
      "worker.queue.depth",
      "worker.queue.pressure",
    ]));

    assert.ok(proxy.requests.length >= 3);
    assert.deepEqual(
      new Set(proxy.requests.map((request) => request.path)),
      new Set(["/v1/logs", "/v1/metrics", "/v1/traces"]),
    );
    assert.ok(proxy.requests.every((request) =>
      request.contentType === "application/json"
      && request.status === 200
    ));

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
      telemetryRequests: proxy.requests.length,
    }, null, 2)}\n`);
  } finally {
    await telemetry.shutdown();
  }
}

async function startRecordingProxy(upstreamBaseUrl) {
  const requests = [];
  const server = createServer((request, response) => {
    void forward(request, response).catch((error) => {
      response.writeHead(502, { "Content-Type": "text/plain" });
      response.end(error instanceof Error ? error.message : String(error));
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.removeListener("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Telemetry proxy did not bind a TCP port.");
  }

  async function forward(request, response) {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const path = request.url ?? "/";
    const contentType = request.headers["content-type"];
    const upstream = await fetch(new URL(path, upstreamBaseUrl), {
      method: request.method,
      headers: {
        ...(contentType === undefined ? {} : { "content-type": contentType }),
        ...(request.headers["content-encoding"] === undefined
          ? {}
          : { "content-encoding": request.headers["content-encoding"] }),
      },
      body,
    });
    const upstreamBody = Buffer.from(await upstream.arrayBuffer());
    requests.push({
      path,
      contentType,
      status: upstream.status,
    });
    response.writeHead(upstream.status, {
      "Content-Type": upstream.headers.get("content-type")
        ?? "application/json",
    });
    response.end(upstreamBody);
  }

  let stopPromise;
  return {
    name: "adaptive-worker-otel-proxy",
    requests,
    url: `http://127.0.0.1:${address.port}`,
    get exited() {
      return !server.listening;
    },
    get unexpectedExit() {
      return undefined;
    },
    stop() {
      stopPromise ??= new Promise((resolve, reject) => {
        server.close((error) => error === undefined ? resolve() : reject(error));
      });
      return stopPromise;
    },
  };
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
