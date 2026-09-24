import assert from "node:assert/strict";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  createFlaggoClient,
  confirmedExposureAttributes,
} from "../../packages/sdk-typescript/dist/index.js";
import {
  InMemoryLogRecordExporter,
  LoggerProvider,
  SimpleLogRecordProcessor,
} from "@opentelemetry/sdk-logs";
import { catalog } from "./dist/catalog.js";
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
        Flaggo__Telemetry__CommitDescriptorPath: resolve(runDirectory, "telemetry", "current.commit.json"),
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
  const client = createFlaggoClient({
    catalog,
    receipt: bootstrap.receipt,
    dataPlaneUrl: dataUrl,
    dataPlaneCredential: { mode: "local-development" },
    availabilityFallback: { mode: "local-default", retries: 0 },
    fetch: forwardingFetch,
  });
  const context = {
    sessionId: "game-phase3",
    userId: "user-phase3",
    cohort: "new_players",
    deviceType: "desktop",
  };
  const highInputs = {
    boardPressure: 0.9,
    recentPlacementTimeMs: 1600,
    recoveryFailures: 3,
    currentLevel: 8,
  };
  const high = await client.tune.numberDetailed("tetris.dropInterval", {
    runtimeTarget: { type: "session", id: "game-phase3" },
    context,
    inputs: highInputs,
  });
  assert.equal(high.source, "server");
  assert.equal(high.confidence, null);
  assert.equal(high.value, 850);
  assert.equal(high.decisionMode, "strategy");
  assert.equal(high.strategyId, "strategy-tetris-balanced-v1");
  assert.equal(high.policy.result, "approved");
  assert.equal(high.fallback.decisionFallbackUsed, false);
  assert.equal(high.exposure.confirmationRequired, true);
  const weightedRule = bootstrap.state.states.find(
    ({ decisionKey }) => decisionKey === "tetris.dropInterval",
  )?.numericRule;
  assert.ok(weightedRule?.weightedInputs?.length === 4);
  const weightedProofs = [];
  const proveWeightedDecision = async ({
    name,
    values,
    expectedScore,
    expectedValue,
    oppositeBoardOnly = false,
  }) => {
    const score = weightedScore(weightedRule, values);
    assert.ok(
      Math.abs(score - expectedScore) < 1e-12,
      `${name} weighted score ${score} did not equal ${expectedScore}`,
    );
    const aggregateAtOrAbove = score >= weightedRule.threshold;
    assert.equal(
      aggregateAtOrAbove,
      expectedValue === weightedRule.valueAtOrAbove,
    );
    if (oppositeBoardOnly) {
      assert.notEqual(
        values["boardPressure"] >= weightedRule.threshold,
        aggregateAtOrAbove,
        `${name} must oppose the board-pressure-only threshold result`,
      );
    }
    const result = await client.tune.numberDetailed(
      "tetris.dropInterval",
      {
        runtimeTarget: { type: "session", id: "game-phase3" },
        context,
        inputs: values,
      },
    );
    assert.equal(result.source, "server");
    assert.equal(result.value, expectedValue);
    assert.equal(result.strategyId, "strategy-tetris-balanced-v1");
    assert.equal(
      result.reason,
      "Applied the active weighted numeric rule strategy.",
    );
    weightedProofs.push({
      name,
      values,
      score,
      expectedValue,
      decisionId: result.decisionId,
    });
    return result;
  };

  await proveWeightedDecision({
    name: "board-high-aggregate-low",
    values: {
      "boardPressure": 0.9,
      "recentPlacementTimeMs": 0,
      "recoveryFailures": 0,
      "currentLevel": 0,
    },
    expectedScore: 0.405,
    expectedValue: 750,
    oppositeBoardOnly: true,
  });
  await proveWeightedDecision({
    name: "board-low-aggregate-high",
    values: {
      "boardPressure": 0.4,
      "recentPlacementTimeMs": 2000,
      "recoveryFailures": 5,
      "currentLevel": 20,
    },
    expectedScore: 0.73,
    expectedValue: 850,
    oppositeBoardOnly: true,
  });

  const sensitivityPairs = [
    [
      {
        name: "placement-time-below",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1200,
          "recoveryFailures": 3,
          "currentLevel": 10,
        },
        expectedScore: 0.545,
        expectedValue: 750,
      },
      {
        name: "placement-time-above",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1240,
          "recoveryFailures": 3,
          "currentLevel": 10,
        },
        expectedScore: 0.55,
        expectedValue: 850,
      },
    ],
    [
      {
        name: "recovery-failures-below",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1000,
          "recoveryFailures": 3,
          "currentLevel": 10,
        },
        expectedScore: 0.52,
        expectedValue: 750,
      },
      {
        name: "recovery-failures-above",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1000,
          "recoveryFailures": 4,
          "currentLevel": 10,
        },
        expectedScore: 0.56,
        expectedValue: 850,
      },
    ],
    [
      {
        name: "current-level-below",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1200,
          "recoveryFailures": 4,
          "currentLevel": 2,
        },
        expectedScore: 0.545,
        expectedValue: 750,
      },
      {
        name: "current-level-above",
        values: {
          "boardPressure": 0.5,
          "recentPlacementTimeMs": 1200,
          "recoveryFailures": 4,
          "currentLevel": 3,
        },
        expectedScore: 0.55,
        expectedValue: 850,
      },
    ],
  ];
  for (const [below, above] of sensitivityPairs) {
    assert.equal(
      below.values["boardPressure"],
      above.values["boardPressure"],
    );
    await proveWeightedDecision(below);
    await proveWeightedDecision(above);
  }

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
    inputs: highInputs,
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
  const exporter = new InMemoryLogRecordExporter();
  const logs = new LoggerProvider({
    processors: [new SimpleLogRecordProcessor({ exporter })],
  });
  try {
    logs.getLogger("tetris", "1").emit({
      eventName: "game.outcome",
      body: { outcome: "recovered", dropIntervalMs: high.value },
      attributes: { ...confirmedExposureAttributes(confirmation), "session.id": context.sessionId },
    });
    await logs.forceFlush();
    await writeFile(
      paths.telemetry,
      exporter.getFinishedLogRecords().map(({ eventName, body, attributes }) =>
        JSON.stringify({ eventName, body, attributes })).join("\n") + "\n",
      { encoding: "utf8", signal: lifecycle.signal },
    );
  } finally {
    await logs.shutdown();
  }

  const recovery = await client.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: {
        boardPressure: 0.2,
        recentPlacementTimeMs: 400,
        recoveryFailures: 0,
        currentLevel: 10,
      },
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
  const requestOnly = await client.tune.numberDetailed(
    "tetris.dropInterval",
    {
      runtimeTarget: { type: "session", id: "game-phase3" },
      context,
      inputs: highInputs,
    },
  );
  assert.equal(requestOnly.source, "server");
  assert.equal(requestOnly.value, 850);
  assert.equal(requestOnly.confidence, null);
  assert.equal(requestOnly.policy.result, "approved");

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
  const unavailable = createFlaggoClient({
    catalog,
    receipt: bootstrap.receipt,
    dataPlaneUrl: unavailableEndpoint.url,
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
    )?.inputKeys.sort(),
    [
      "boardPressure",
      "currentLevel",
      "recentPlacementTimeMs",
      "recoveryFailures",
    ],
  );
  const auditedCooldown = inspection.policyResults.find(
    ({ decisionId }) => decisionId === cooldown.decisionId,
  );
  assert.equal(auditedCooldown?.result, "blocked");
  assert.equal(auditedCooldown?.fallbackSource, "server");
  for (const proof of weightedProofs) {
    const detail = inspection.decisionDetails.find(
      ({ decisionId }) => decisionId === proof.decisionId,
    );
    assert.deepEqual(detail?.inputs, proof.values);
    assert.equal(detail?.strategyId, "strategy-tetris-balanced-v1");
    assert.equal(
      detail?.reason,
      "Applied the active weighted numeric rule strategy.",
    );
  }
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
    requestInputsIndependentOfEvidence: true,
    requestOnlyMs: requestOnly.value,
    weightedProofs: weightedProofs.map(({ name, score, expectedValue }) => ({
      name,
      score,
      expectedValue,
    })),
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

function weightedScore(rule, values) {
  return rule.weightedInputs.reduce((score, weightedInput) => {
    const value = values[weightedInput.inputKey];
    assert.equal(typeof value, "number");
    const normalized = Math.min(
      1,
      Math.max(
        0,
        (value - weightedInput.minimum) /
        (weightedInput.maximum - weightedInput.minimum),
      ),
    );
    return score + normalized * weightedInput.weight;
  }, 0);
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
