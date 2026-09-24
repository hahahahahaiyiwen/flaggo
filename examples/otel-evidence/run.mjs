import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { createFlaggoClient, confirmedExposureAttributes, FlaggoHttpError } from "@flaggo/sdk";
import { publishLocalManifest } from "../shared/publish-local-manifest.mjs";
import { publishJsonGeneration } from "../tetris-integration/durable-json.mjs";
import {
  createHostLifecycle, installSignalHandlers, runWithCleanup, startControlAndDataHosts,
  startHost, startManagedProcess, waitForReady,
} from "../tetris-integration/host-process.mjs";
import { inspectIntegration } from "../tetris-integration/inspect.mjs";
import { catalog } from "./dist/catalog.js";
import { createApplicationTelemetry } from "./telemetry.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(repositoryRoot, ".flaggo", `otel-evidence-${process.pid}-${Date.now()}`);
const execute = promisify(execFile);
const collectorVersion = "0.161.0";
const decisionKey = "worker.batchSize";
const paths = {
  registry: resolve(runDirectory, "registry.json"),
  bootstrap: resolve(runDirectory, "bootstrap"),
  audit: resolve(runDirectory, "audit.jsonl"),
  telemetry: resolve(runDirectory, "telemetry.jsonl"),
  inputs: resolve(runDirectory, "inputs", "current.commit.json"),
  controlLog: resolve(runDirectory, "control.log"),
  dataLog: resolve(runDirectory, "data.log"),
  collectorLog: resolve(runDirectory, "collector.log"),
};

async function run(lifecycle, collector) {
  const bundle = JSON.parse(await readFile(resolve(exampleDirectory, "generated", "definitions.json"), "utf8"));
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, { ...init, signal: init.signal ?? lifecycle.signal });
  const common = {
    Flaggo__Authentication__LocalDevelopmentAppId: bundle.application.id,
    Flaggo__Authentication__LocalDevelopmentEnvironment: bundle.application.environment,
    Flaggo__Registry__LocalFilePath: paths.registry,
  };
  const runtime = await startControlAndDataHosts({
    lifecycle,
    createControlHost: () => startHost("control-plane",
      resolve(repositoryRoot, "apps/control-plane/src/Flaggo.ControlPlane/bin/Debug/net10.0/Flaggo.ControlPlane.dll"),
      paths.controlLog, repositoryRoot, common),
    waitForControlReady: (url, host, signal) => waitForReady(async (probeSignal) => {
      const response = await fetchWithAbort(`${url}/v1/definition-bundles:validate`, {
        method: "POST",
        headers: { Authorization: "Flaggo-Local-Development", "Content-Type": "application/json" },
        body: JSON.stringify(bundle), signal: probeSignal,
      });
      if (!response.ok) throw new Error(`Manifest validation failed: ${await response.text()}`);
      return true;
    }, host, signal),
    bootstrap: async (url, signal) => {
      const result = await publishLocalManifest({ controlPlaneUrl: url, bundle, fetch: fetchWithAbort });
      const accepted = result.receipt.acceptedDefinitions[decisionKey];
      await publishJsonGeneration(paths.bootstrap, {
        receipt: result.receipt,
        evidence: { version: 1, evidenceByStrategy: {} },
        state: {
          version: 1,
          states: [{
            decisionKey, ...accepted, value: 3,
            controlTarget: { type: "global", id: "global" },
            mode: "strategy", strategyId: "observed-workload",
            numericRule: {
              inputKey: "pressure", threshold: 0.5, valueAtOrAbove: 6, valueBelow: 3,
              weightedInputs: [
                { inputKey: "pressure", minimum: 0, maximum: 1, weight: 0.7 },
                { inputKey: "durationMs", minimum: 0, maximum: 1000, weight: 0.1 },
                { inputKey: "retries", minimum: 0, maximum: 10, weight: 0.1 },
                { inputKey: "failures", minimum: 0, maximum: 10, weight: 0.1 },
              ],
            },
          }],
        },
      }, signal);
      return result;
    },
    createDataHost: () => startHost("data-plane",
      resolve(repositoryRoot, "apps/data-plane/src/Flaggo.DataPlane/bin/Debug/net10.0/Flaggo.DataPlane.dll"),
      paths.dataLog, repositoryRoot, {
        ...common,
        Flaggo__Bootstrap__LocalGenerationPath: paths.bootstrap,
        Flaggo__Audit__LocalFilePath: paths.audit,
        Flaggo__Telemetry__CommitDescriptorPath: paths.inputs,
      }),
    waitForDataReady: (url, host, signal) => waitForReady(async (probeSignal) => {
      const response = await fetchWithAbort(`${url}/health/ready`, { signal: probeSignal });
      if (!response.ok) throw new Error(`Data-plane readiness failed: ${await response.text()}`);
      return (await response.json()).status === "ready";
    }, host, signal),
  });
  const client = createFlaggoClient({
    catalog,
    receipt: runtime.bootstrap.receipt,
    dataPlaneUrl: runtime.dataUrl,
    dataPlaneCredential: { mode: "local-development" },
    availabilityFallback: { mode: "local-default", retries: 0 },
    fetch: fetchWithAbort,
  });
  const context = { sessionId: "worker-session" };
  const request = { context, idempotencyKey: "collector-evidence" };
  await assert.rejects(() => client.tune.numberDetailed(decisionKey, request), (error) => {
    assert.ok(error instanceof FlaggoHttpError);
    assert.equal(error.problem.code, "required-evidence-unavailable");
    assert.equal(error.problem.clientFallback.eligible, false);
    return true;
  });

  const port = await availablePort();
  const collectorUrl = `http://127.0.0.1:${port}`;
  const collectorEnvironment = {
    ...process.env,
    OTEL_RECEIVER_ENDPOINT: `127.0.0.1:${port}`,
    FLAGGO_OTLP_BASE_URL: `${runtime.dataUrl}/otlp/otel-worker/dev`,
    FLAGGO_OTLP_AUTHORIZATION: "Flaggo-Local-Telemetry",
  };
  const config = resolve(exampleDirectory, "collector.yaml");
  await execute(collector, ["validate", "--config", config], { env: collectorEnvironment, timeout: 30000 });
  const collectorHost = lifecycle.startHost(() => startManagedProcess(
    "opentelemetry-collector", collector, ["--config", config],
    paths.collectorLog, repositoryRoot, collectorEnvironment, collectorUrl,
  ));
  await collectorHost.waitForListening({ signal: lifecycle.signal });
  await waitForReady(async (signal) => {
    const response = await fetchWithAbort(`${collectorUrl}/v1/metrics`, {
      method: "POST", headers: { "Content-Type": "application/x-protobuf" },
      body: new Uint8Array(), signal,
    });
    if (!response.ok) throw new Error(`Collector readiness failed: ${await response.text()}`);
    return true;
  }, collectorHost, lifecycle.signal);

  const telemetry = createApplicationTelemetry(collectorUrl);
  try {
    const span = telemetry.emitWorkload(context.sessionId);
    await telemetry.flush();
    let decision;
    await waitForReady(async () => {
      try {
        decision = await client.tune.numberDetailed(decisionKey, request);
        return true;
      } catch (error) {
        if (error instanceof FlaggoHttpError && error.problem.code === "required-evidence-unavailable") return false;
        throw error;
      }
    }, runtime.data, lifecycle.signal);
    assert.equal(decision.source, "server");
    assert.equal(decision.value, 6);
    assert.equal(decision.confidence, null);
    await writeFile(paths.telemetry, "", "utf8");
    const beforeConfirmation = await inspectIntegration(paths.audit, paths.telemetry);
    assert.equal(beforeConfirmation.exposureCount, 0);
    const audit = beforeConfirmation.decisionDetails.find((record) => record.decisionId === decision.decisionId);
    assert.deepEqual(audit.inputs, { pressure: 0.75, durationMs: 250, retries: 2, failures: 2 });
    assert.deepEqual(audit.requestInputs, {});
    assert.equal(new Set(Object.values(audit.inputProvenance).map((value) => value.generation)).size, 1);
    assert.equal(audit.inputProvenance.durationMs.traceId, span.traceId);
    assert.equal(audit.inputProvenance.retries.spanId, span.spanId);
    for (const input of Object.values(audit.inputProvenance)) {
      assert.equal(input.source, "evidence");
      assert.equal(input.coverage, "observed");
      assert.ok(BigInt(input.observedTimeUnixNano) > 0n);
    }

    const queue = Array.from({ length: 8 }, (_, index) => index);
    const processed = queue.splice(0, decision.value);
    const confirmation = await client.exposures.confirm(decision.decisionId, decision.exposure.confirmToken);
    telemetry.logger.emit({
      eventName: "work.completed",
      body: "Batch applied.",
      attributes: {
        "session.id": context.sessionId,
        "processed.count": processed.length,
        ...confirmedExposureAttributes(confirmation),
      },
    });
    await telemetry.flush();
    let outcome;
    await waitForReady(async () => {
      const snapshot = await readCommittedInputs();
      outcome = snapshot.frames.find((frame) => frame.key.binding === "appliedOutcome");
      return outcome?.status === "available" && outcome.exposureId === confirmation.exposureId;
    }, runtime.data, lifecycle.signal);
    assert.equal(outcome.value, 6);
    assert.equal(outcome.key.scope.tenantId, "local-development");
    assert.equal(outcome.key.definitionId, decision.definition.definitionId);
    assert.deepEqual(outcome.key.target, { type: "session", id: context.sessionId });
    assert.deepEqual(await client.tune.numberDetailed(decisionKey, request), decision);
    await writeFile(paths.telemetry, telemetry.captured.getFinishedLogRecords()
      .map(({ eventName, body, attributes }) => JSON.stringify({ eventName, body, attributes })).join("\n"), "utf8");
    const inspection = await inspectIntegration(paths.audit, paths.telemetry);
    assert.equal(inspection.decisionCount, 1);
    assert.equal(inspection.exposureCount, 1);
    const mirrored = await readFile(paths.collectorLog, "utf8");
    assert.ok(mirrored.includes("work.queue.pressure"));
    assert.ok(mirrored.includes("work.process"));
    assert.ok(mirrored.includes("work.summary"));
    lifecycle.assertHealthy();
    process.stdout.write(`${JSON.stringify({
      status: "passed", collectorVersion, batchSize: decision.value,
      resolvedInputs: audit.inputs, snapshotGeneration: audit.inputProvenance.pressure.generation,
      exposureId: confirmation.exposureId, correlatedOutcome: outcome.value,
      existingPipelinesPreserved: true,
    }, null, 2)}\n`);
  } finally {
    await telemetry.stop();
  }
}

async function availablePort() {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address();
  await new Promise((done, reject) => server.close((error) => error ? reject(error) : done()));
  return port;
}

async function readCommittedInputs() {
  const descriptor = JSON.parse(await readFile(paths.inputs, "utf8"));
  assert.equal(descriptor.format, "flaggo.committed-artifact");
  assert.equal(descriptor.version, 1);
  assert.match(descriptor.artifact, /^input-evidence-[a-f0-9]{32}\.json$/u);
  const bytes = await readFile(resolve(dirname(paths.inputs), descriptor.artifact));
  assert.equal(bytes.length, descriptor.byteLength);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), descriptor.sha256);
  return JSON.parse(bytes);
}

async function main() {
  const collector = process.env.FLAGGO_OTELCOL_PATH;
  if (!collector) throw new Error(`Set FLAGGO_OTELCOL_PATH to the stock otelcol ${collectorVersion} executable.`);
  const { stdout } = await execute(collector, ["--version"], { timeout: 10000 });
  assert.equal(stdout.trim(), `otelcol version ${collectorVersion}`);
  await mkdir(dirname(runDirectory), { recursive: true });
  await mkdir(runDirectory);
  const lifecycle = createHostLifecycle({ removeRunDirectory: () => rm(runDirectory, { recursive: true, force: true }) });
  const uninstall = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(async () => {
      try {
        await run(lifecycle, collector);
      } catch (error) {
        for (const path of [paths.controlLog, paths.dataLog, paths.collectorLog]) {
          try {
            process.stderr.write(`${path}\n${(await readFile(path, "utf8")).slice(-6000)}\n`);
          } catch (readError) {
            if (readError.code !== "ENOENT") process.stderr.write(`Cannot read diagnostics: ${readError}\n`);
          }
        }
        throw error;
      }
    }, lifecycle);
  } finally {
    uninstall();
  }
}

await main();
