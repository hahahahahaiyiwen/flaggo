import assert from "node:assert/strict";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  FlaggoHttpError,
  createFlaggoClient,
  createSignalHandle,
} from "../../packages/sdk-typescript/dist/index.js";
import {
  bootstrapTetris,
  loadCanonicalBundle,
  publishJsonGeneration,
} from "./bootstrap.mjs";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
  startControlAndDataHosts,
  startHost,
  startUnavailableEndpoint,
  waitForReady,
} from "./host-process.mjs";
import { inspectIntegration } from "./inspect.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `integration-${process.pid}-${Date.now()}`,
);
const paths = {
  registry: resolve(runDirectory, "definition-registry-v1.json"),
  bootstrap: resolve(runDirectory, "bootstrap"),
  audit: resolve(runDirectory, "audit.jsonl"),
  telemetry: resolve(runDirectory, "telemetry.jsonl"),
  controlLog: resolve(runDirectory, "control-plane.log"),
  dataLog: resolve(runDirectory, "data-plane.log"),
};

async function runIntegration(lifecycle) {
  await mkdir(runDirectory, { recursive: true });
  lifecycle.signal.throwIfAborted();
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, { ...init, signal: init.signal ?? lifecycle.signal });

  try {
  const bundle = await loadCanonicalBundle(lifecycle.signal);
  lifecycle.signal.throwIfAborted();
  const runtime = await startControlAndDataHosts({
    lifecycle,
    createControlHost: () => startHost(
      "control-plane",
      resolve(
        repositoryRoot,
        "apps/control-plane/src/Flaggo.ControlPlane/bin/Debug/net10.0/Flaggo.ControlPlane.dll",
      ),
      paths.controlLog,
      repositoryRoot,
      {
        Flaggo__Registry__LocalFilePath: paths.registry,
      },
    ),
    waitForControlReady: (controlUrl, control, signal) =>
      waitForReady(async (probeSignal) => {
        const response = await fetchWithAbort(
          `${controlUrl}/v1/definition-bundles:validate`,
          {
            method: "POST",
            headers: {
              Authorization: "Flaggo-Local-Development",
              "Content-Type": "application/json",
            },
            body: JSON.stringify(bundle),
            signal: probeSignal,
          },
        );
        const body = await response.text();
        if (!response.ok) {
          throw new Error(
            `control-plane readiness returned HTTP ${response.status}: ${body}`,
          );
        }
        return true;
      }, control, signal),
    bootstrap: (controlUrl, signal) => bootstrapTetris({
      controlPlaneUrl: controlUrl,
      dataPlaneUrl: "http://127.0.0.1:0",
      publicationPath: paths.bootstrap,
      fetchImpl: fetchWithAbort,
      signal,
    }),
    createDataHost: () => startHost(
      "data-plane",
      resolve(
        repositoryRoot,
        "apps/data-plane/src/Flaggo.DataPlane/bin/Debug/net10.0/Flaggo.DataPlane.dll",
      ),
      paths.dataLog,
      repositoryRoot,
      {
        Flaggo__Registry__LocalFilePath: paths.registry,
        Flaggo__Bootstrap__LocalGenerationPath: paths.bootstrap,
        Flaggo__Audit__LocalFilePath: paths.audit,
      },
    ),
    waitForDataReady: (dataUrl, data, signal) =>
      waitForReady(async (probeSignal) => {
        const response = await fetchWithAbort(
          `${dataUrl}/health/ready`,
          { signal: probeSignal },
        );
        const body = await response.text();
        if (!response.ok) {
          throw new Error(
            `data-plane readiness returned HTTP ${response.status}: ${body}`,
          );
        }
        const readiness = JSON.parse(body);
        if (readiness.status !== "ready") {
          throw new Error(
            `data-plane readiness reported '${readiness.status}': ${body}`,
          );
        }
        return true;
      }, data, signal),
  });
  const {
    bootstrap,
    controlUrl,
    dataUrl,
  } = runtime;
  assert.equal(
    bootstrap.approvalRequired,
    true,
    "clean Phase 3 bootstrap must exercise typed approval",
  );

  const capturedBodies = [];
  const forwardingFetch = async (input, init) => {
    if (String(input).includes("/v1/decisions/")) {
      capturedBodies.push(JSON.parse(String(init?.body)));
    }
    return fetchWithAbort(input, init);
  };
  const client = await createFlaggoClient({
    appId: "tetris-demo",
    environment: "dev",
    dataPlaneUrl: dataUrl,
    dataPlaneCredential: { mode: "local-development" },
    controlPlane: {
      mode: "pre-registered",
      receipt: bootstrap.receipt,
      bundle,
    },
    availabilityFallback: { mode: "local-default", retries: 0 },
    fetch: forwardingFetch,
  });
  const context = {
    sessionId: "game-phase3",
    userId: "user-phase3",
    cohort: "new_players",
    deviceType: "desktop",
  };
  const highInputs = [
    input("tetris.boardPressure", 0.9),
    input("tetris.recentPlacementTimeMs", 1600),
    input("tetris.recoveryFailures", 3),
    input("tetris.currentLevel", 8),
  ];
  const high = await client.tune.numberDetailed("tetris.dropInterval", {
    runtimeTarget: { type: "session", id: "game-phase3" },
    context,
    inputs: highInputs,
  });
  assert.equal(high.source, "server");
  assert.equal(high.value, 850);
  assert.equal(high.decisionMode, "strategy");
  assert.equal(high.strategyId, "strategy-tetris-balanced-v1");
  assert.equal(high.policy.result, "approved");
  assert.equal(high.fallback.decisionFallbackUsed, false);
  assert.equal(high.exposure.confirmationRequired, true);

  const sdkPackage = JSON.parse(
    await readFile(
      resolve(repositoryRoot, "packages/sdk-typescript/package.json"),
      "utf8",
    ),
  );
  const accepted =
    bootstrap.receipt.acceptedDefinitions["tetris.dropInterval"];
  const directBody = {
    expectedContract: {
      ...accepted,
      bundleDigest: bootstrap.receipt.bundleDigest,
      buildId: bootstrap.receipt.buildId,
    },
    runtimeTarget: { type: "session", id: "game-phase3" },
    runtimeContext: context,
    inputs: [...highInputs].sort((left, right) =>
      left.signal.key.localeCompare(right.signal.key, "en", {
        usage: "sort",
        sensitivity: "variant",
      })
    ),
    client: {
      appId: "tetris-demo",
      environment: "dev",
      sdk: "typescript",
      sdkVersion: sdkPackage.version,
    },
  };
  assert.deepEqual(capturedBodies[0], directBody);
  const directResponse = await fetchWithAbort(
    `${dataUrl}/v1/decisions/tetris.dropInterval:decide`,
    {
      method: "POST",
      headers: {
        Authorization: "Flaggo-Local-Development",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(directBody),
    },
  );
  assert.equal(directResponse.status, 200);
  const direct = await directResponse.json();
  assert.deepEqual(contractProjection(direct), contractProjection(high));

  const confirmation = await client.exposures.confirm(
    high.decisionId,
    high.exposure.confirmToken,
    { appliedAt: new Date().toISOString() },
  );
  const telemetryEvents = [];
  const outcomeDeclaration = bundle.signals.find(
    ({ key }) => key === "tetris.outcomeObserved",
  );
  const outcome = createSignalHandle(outcomeDeclaration, {
    emit(event) {
      telemetryEvents.push(event);
    },
  });
  outcome.emit({
    decisionId: high.decisionId,
    exposureId: confirmation.exposureId,
    outcome: "recovered",
    dropIntervalMs: high.value,
  });
  await writeFile(
    paths.telemetry,
    telemetryEvents.map((event) => JSON.stringify(event)).join("\n") + "\n",
    { encoding: "utf8", signal: lifecycle.signal },
  );

  const recovery = await client.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: [
        input("tetris.boardPressure", 0.2),
        input("tetris.recentPlacementTimeMs", 400),
        input("tetris.recoveryFailures", 0),
        input("tetris.currentLevel", 10),
      ],
    },
  );
  assert.equal(recovery.source, "server");
  assert.equal(recovery.value, 750);
  assert.equal(recovery.policy.result, "approved");

  await publishJsonGeneration(
    bootstrap.publication.rootPath,
    {
      receipt: bootstrap.receipt,
      state: bootstrap.state,
      evidence: {
        version: 1,
        evidenceByStrategy: {},
      },
    },
    lifecycle.signal,
  );
  const evidenceRecoveryIdempotencyKey = "integration-evidence-recovery";
  try {
    await assert.rejects(
      () => client.tune.numberDetailed(
        "tetris.dropInterval",
        {
          runtimeTarget: { type: "session", id: "game-phase3" },
          context,
          inputs: highInputs,
          idempotencyKey: evidenceRecoveryIdempotencyKey,
        },
      ),
      (error) => {
        assert.ok(error instanceof FlaggoHttpError);
        assert.equal(error.problem.code, "required-evidence-unavailable");
        assert.equal(error.problem.clientFallback?.eligible, false);
        return true;
      },
    );
  } finally {
    await publishJsonGeneration(
      bootstrap.publication.rootPath,
      {
        receipt: bootstrap.receipt,
        state: bootstrap.state,
        evidence: bootstrap.evidence,
      },
      lifecycle.signal,
    );
  }
  const evidenceRecovered = await client.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: highInputs,
      idempotencyKey: evidenceRecoveryIdempotencyKey,
    },
  );
  assert.equal(evidenceRecovered.source, "server");
  assert.equal(evidenceRecovered.value, 850);
  assert.equal(evidenceRecovered.policy.result, "approved");

  const coolingState = structuredClone(bootstrap.state);
  coolingState.states[0].lastChangedAt = new Date().toISOString();
  await publishJsonGeneration(
    bootstrap.publication.rootPath,
    {
      receipt: bootstrap.receipt,
      state: coolingState,
      evidence: bootstrap.evidence,
    },
    lifecycle.signal,
  );
  const cooldown = await client.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: highInputs,
    },
  );
  assert.equal(cooldown.source, "server");
  assert.equal(cooldown.value, 800);
  assert.equal(cooldown.policy.result, "blocked");
  assert.ok(cooldown.policy.reasons.includes("cooldown_active"));
  assert.equal(cooldown.fallback.source, "server");
  assert.equal(cooldown.fallback.decisionFallbackUsed, true);
  assert.equal(cooldown.exposure.confirmationRequired, false);

  const unavailableEndpoint = await lifecycle.startHostAsync(
    startUnavailableEndpoint,
  );
  const unavailable = await createFlaggoClient({
    appId: "tetris-demo",
    environment: "dev",
    dataPlaneUrl: unavailableEndpoint.url,
    controlPlane: {
      mode: "pre-registered",
      receipt: bootstrap.receipt,
      bundle,
    },
    availabilityFallback: { mode: "local-default", retries: 0 },
    fetch: fetchWithAbort,
  });
  const clientFallback = await unavailable.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: highInputs,
    },
  );
  assert.equal(clientFallback.source, "client-fallback");
  assert.equal(clientFallback.value, 800);
  assert.equal(clientFallback.fallback.source, "client-fallback");
  assert.equal("decisionId" in clientFallback, false);

  const inspection = await inspectIntegration(paths.audit, paths.telemetry);
  assert.equal(inspection.exposureCount, 1);
  assert.equal(inspection.linkedOutcomeCount, 1);
  assert.ok(
    inspection.strategies.includes("strategy-tetris-balanced-v1"),
  );
  assert.equal(
    inspection.exposures.some(
      ({ decisionId }) => decisionId === direct.decisionId,
    ),
    false,
    "unused direct REST receipt must not create an exposure record",
  );
  const auditedHigh = inspection.policyResults.find(
    ({ decisionId }) => decisionId === high.decisionId,
  );
  assert.equal(auditedHigh?.result, "approved");
  assert.deepEqual(
    inspection.decisionInputs.find(
      ({ decisionId }) => decisionId === high.decisionId,
    )?.signalKeys.sort(),
    [
      "tetris.boardPressure",
      "tetris.currentLevel",
      "tetris.recentPlacementTimeMs",
      "tetris.recoveryFailures",
    ],
  );
  const auditedCooldown = inspection.policyResults.find(
    ({ decisionId }) => decisionId === cooldown.decisionId,
  );
  assert.equal(auditedCooldown?.result, "blocked");
  assert.equal(auditedCooldown?.fallbackSource, "server");
  lifecycle.assertHealthy();

  process.stdout.write(`${JSON.stringify({
    status: "passed",
    approvalRequired: bootstrap.approvalRequired,
    sdkRestContractEquivalent: true,
    highPressureMs: high.value,
    recoveryMs: recovery.value,
    cooldownFallbackMs: cooldown.value,
    serverFallbackSource: cooldown.fallback.source,
    clientFallbackMs: clientFallback.value,
    clientFallbackSource: clientFallback.fallback.source,
    missingEvidenceFailClosed: true,
    evidenceRecoveryMs: evidenceRecovered.value,
    exposureCount: inspection.exposureCount,
    linkedOutcomeCount: inspection.linkedOutcomeCount,
    unusedReceiptDecisionId: direct.decisionId,
  }, null, 2)}\n`);
} catch (error) {
  if (!lifecycle.signal.aborted) {
    const logs = {};
    for (const [name, path] of [
      ["control", paths.controlLog],
      ["data", paths.dataLog],
    ]) {
      try {
        logs[name] = (await readFile(path, "utf8")).slice(-4000);
      } catch {
        logs[name] = "";
      }
    }
    error.message = `${error.message}\nHost logs:\n${JSON.stringify(logs, null, 2)}`;
  }
  throw error;
  }
}

function input(key, value) {
  return { signal: { key }, value };
}

function contractProjection(result) {
  return {
    value: result.value,
    valueType: result.valueType,
    decisionMode: result.decisionMode,
    strategyId: result.strategyId,
    definition: result.definition,
    targetProvenance: result.targetProvenance,
    resolutionChain: result.resolutionChain,
    fallback: result.fallback,
    policy: result.policy,
    definitionStatus: result.definitionStatus,
    reason: result.reason,
  };
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () => rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => runIntegration(lifecycle),
      lifecycle,
      (cleanupError) => {
        process.stderr.write(
          `Integration cleanup failed without replacing the workflow failure: `
          + `${formatError(cleanupError)}\n`,
        );
      },
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
  if (error instanceof AggregateError) {
    return [
      error.stack ?? error.message,
      ...error.errors.map(
        (failure, index) => `Cleanup failure ${index + 1}: ${formatError(failure)}`,
      ),
    ].join("\n");
  }
  if (error instanceof Error) {
    const formatted = error.stack ?? error.message;
    return error.cause === undefined
      ? formatted
      : `${formatted}\nCaused by: ${formatError(error.cause)}`;
  }
  return String(error);
}

if (
  process.argv[1] !== undefined
  && pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
