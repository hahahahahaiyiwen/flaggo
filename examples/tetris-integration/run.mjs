import assert from "node:assert/strict";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { once } from "node:events";

import {
  FlaggoHttpError,
  createFlaggoClient,
  createSignalHandle,
} from "../../packages/sdk-typescript/dist/index.js";
import { bootstrapTetris, loadCanonicalBundle, writeJsonAtomic } from "./bootstrap.mjs";
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
  state: resolve(runDirectory, "governed-state-v1.json"),
  audit: resolve(runDirectory, "audit.jsonl"),
  evidence: resolve(runDirectory, "evidence.json"),
  receipt: resolve(runDirectory, "registration-receipt.json"),
  telemetry: resolve(runDirectory, "telemetry.jsonl"),
  controlLog: resolve(runDirectory, "control-plane.log"),
  dataLog: resolve(runDirectory, "data-plane.log"),
};

await mkdir(runDirectory, { recursive: true });
const hosts = [];
try {
  const [controlPort, dataPort, unavailablePort] = await Promise.all([
    availablePort(),
    availablePort(),
    availablePort(),
  ]);
  const controlUrl = `http://127.0.0.1:${controlPort}`;
  const dataUrl = `http://127.0.0.1:${dataPort}`;
  const bundle = await loadCanonicalBundle();
  const canonicalEvidence = await readFile(
    resolve(exampleDirectory, "evidence.json"),
    "utf8",
  );
  await writeFile(
    paths.evidence,
    canonicalEvidence,
    "utf8",
  );
  const control = startHost(
    "control-plane",
    resolve(
      repositoryRoot,
      "apps/control-plane/src/Flaggo.ControlPlane/bin/Debug/net10.0/Flaggo.ControlPlane.dll",
    ),
    controlUrl,
    paths.controlLog,
    {
      Flaggo__Registry__LocalFilePath: paths.registry,
    },
  );
  hosts.push(control);
  await waitFor(async () => {
    const response = await fetch(
      `${controlUrl}/v1/definition-bundles:validate`,
      {
        method: "POST",
        headers: {
          Authorization: "Flaggo-Local-Development",
          "Content-Type": "application/json",
        },
        body: JSON.stringify(bundle),
      },
    );
    return response.ok;
  }, control);

  const bootstrap = await bootstrapTetris({
    controlPlaneUrl: controlUrl,
    dataPlaneUrl: dataUrl,
    statePath: paths.state,
    receiptPath: paths.receipt,
  });
  assert.equal(
    bootstrap.approvalRequired,
    true,
    "clean Phase 3 bootstrap must exercise typed approval",
  );

  const data = startHost(
    "data-plane",
    resolve(
      repositoryRoot,
      "apps/data-plane/src/Flaggo.DataPlane/bin/Debug/net10.0/Flaggo.DataPlane.dll",
    ),
    dataUrl,
    paths.dataLog,
    {
      Flaggo__Registry__LocalFilePath: paths.registry,
      Flaggo__State__LocalFilePath: paths.state,
      Flaggo__Evidence__LocalFilePath: paths.evidence,
      Flaggo__Audit__LocalFilePath: paths.audit,
    },
  );
  hosts.push(data);
  await waitFor(async () => {
    const response = await fetch(`${dataUrl}/health/ready`);
    if (!response.ok) return false;
    return (await response.json()).status === "ready";
  }, data);

  const capturedBodies = [];
  const forwardingFetch = async (input, init) => {
    if (String(input).includes("/v1/decisions/")) {
      capturedBodies.push(JSON.parse(String(init?.body)));
    }
    return fetch(input, init);
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
  const directResponse = await fetch(
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
    "utf8",
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

  await writeJsonAtomic(paths.evidence, {
    version: 1,
    evidenceByStrategy: {},
  });
  try {
    await assert.rejects(
      () => client.tune.numberDetailed(
        "tetris.dropInterval",
        {
          runtimeTarget: { type: "session", id: "game-phase3" },
          context,
          inputs: highInputs,
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
    await writeJsonAtomic(paths.evidence, JSON.parse(canonicalEvidence));
  }

  const coolingState = structuredClone(bootstrap.state);
  coolingState.states[0].lastChangedAt = new Date().toISOString();
  await writeJsonAtomic(paths.state, coolingState);
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

  const unavailable = await createFlaggoClient({
    appId: "tetris-demo",
    environment: "dev",
    dataPlaneUrl: `http://127.0.0.1:${unavailablePort}`,
    controlPlane: {
      mode: "pre-registered",
      receipt: bootstrap.receipt,
      bundle,
    },
    availabilityFallback: { mode: "local-default", retries: 0 },
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
    exposureCount: inspection.exposureCount,
    linkedOutcomeCount: inspection.linkedOutcomeCount,
    unusedReceiptDecisionId: direct.decisionId,
  }, null, 2)}\n`);
} catch (error) {
  for (const host of hosts) {
    await host.stop();
  }
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
  throw error;
} finally {
  for (const host of hosts) {
    await host.stop();
  }
  if (process.env.FLAGGO_KEEP_INTEGRATION_FILES !== "1") {
    await rm(runDirectory, { recursive: true, force: true });
  } else {
    process.stderr.write(`Integration files retained at ${runDirectory}\n`);
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

async function availablePort() {
  const server = createServer();
  server.unref();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  const port = address.port;
  server.close();
  await once(server, "close");
  return port;
}

function startHost(name, assembly, url, logPath, configuration) {
  const log = createWriteStream(logPath, { flags: "a" });
  const child = spawn("dotnet", [assembly], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      ASPNETCORE_ENVIRONMENT: "Development",
      ASPNETCORE_URLS: url,
      Flaggo__Authentication__LocalDevelopmentBypass: "true",
      ...configuration,
    },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  let exited = false;
  child.once("exit", () => {
    exited = true;
    log.end();
  });
  return {
    name,
    get exited() {
      return exited;
    },
    async stop() {
      if (exited) return;
      child.kill();
      await Promise.race([
        once(child, "exit"),
        new Promise((resolvePromise) => setTimeout(resolvePromise, 3000)),
      ]);
      if (!exited) {
        child.kill("SIGKILL");
        await once(child, "exit");
      }
    },
  };
}

async function waitFor(probe, host) {
  const deadline = Date.now() + 30000;
  let lastError;
  while (Date.now() < deadline) {
    if (host.exited) {
      throw new Error(`${host.name} exited before becoming ready.`);
    }
    try {
      if (await probe()) return;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }
  throw new Error(
    `${host.name} did not become ready: ${lastError?.message ?? "timeout"}`,
  );
}
