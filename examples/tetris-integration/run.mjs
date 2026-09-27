import assert from "node:assert/strict";
import { mkdir, readFile, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createContractServiceClient,
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
  startContractAndDecisionHosts,
  startHost,
  waitForReady,
} from "./host-process.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `tetris-integration-${process.pid}-${Date.now()}`,
);

async function runIntegration(lifecycle) {
  await mkdir(runDirectory, { recursive: true });
  const contract = JSON.parse(
    await readFile(resolve(exampleDirectory, "decision-contract.json"), {
      encoding: "utf8",
      signal: lifecycle.signal,
    }),
  );
  const paths = {
    database: resolve(runDirectory, "flaggo.db"),
    contractLog: resolve(runDirectory, "contract-service.log"),
    decisionLog: resolve(runDirectory, "decision-service.log"),
  };
  const commonConfiguration = {
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
    Flaggo__Authentication__Application: "tetris",
    Flaggo__Authentication__Environment: "integration",
  };
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal ?? lifecycle.signal,
    });
  const runtime = await startContractAndDecisionHosts({
    lifecycle,
    createContractHost: () => startHost(
      "tetris-contract-service",
      resolve(
        repositoryRoot,
        "apps/contract-service/src/Flaggo.ContractService/bin/Debug/net10.0/Flaggo.ContractService.dll",
      ),
      paths.contractLog,
      repositoryRoot,
      commonConfiguration,
    ),
    waitForContractReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
    publishContract: async (url) => {
      const management = createContractServiceClient({
        contractServiceUrl: url,
        credential: { mode: "local-development" },
        fetch: fetchWithAbort,
      });
      const validation = await management.validate(contract.name, contract);
      assert.equal(validation.status, "valid");
      return management.put(contract.name, contract);
    },
    createDecisionHost: () => startHost(
      "tetris-decision-service",
      resolve(
        repositoryRoot,
        "apps/decision-service/src/Flaggo.DecisionService/bin/Debug/net10.0/Flaggo.DecisionService.dll",
      ),
      paths.decisionLog,
      repositoryRoot,
      commonConfiguration,
    ),
    waitForDecisionReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
  });

  const capturedBodies = [];
  const forwardingFetch = async (input, init) => {
    if (String(input).includes("/v3/decision-contracts/")) {
      capturedBodies.push(JSON.parse(String(init?.body)));
    }
    return fetchWithAbort(input, init);
  };
  const client = createFlaggoClient({
    decisionServiceUrl: runtime.decisionUrl,
    contracts: {
      [contract.name]: {
        contractDigest: runtime.publication.contractDigest,
      },
    },
    credential: { mode: "local-development" },
    fetch: forwardingFetch,
    random: () => 0.125,
  });
  const common = {
    current_level: 8,
    recent_placement_time_ms: 1600,
    session_id: "game-v3",
  };
  const high = await client.decide(contract.name, {
    attributes: {
      ...common,
      board_pressure: 0.9,
      recovery_failures: 3,
    },
  });
  const low = await client.decide(contract.name, {
    attributes: {
      ...common,
      board_pressure: 0.4,
      recovery_failures: 0,
    },
  });
  const missing = await client.decide(contract.name, {
    attributes: {
      session_id: "game-v3",
    },
  });

  assert.equal(high.result, 850);
  assert.deepEqual(high.evaluation, { source: "rule", rule: "high-pressure" });
  assert.equal(low.result, 750);
  assert.deepEqual(low.evaluation, { source: "rule", rule: "low-pressure" });
  assert.equal(missing.result, 800);
  assert.deepEqual(missing.evaluation, { source: "default" });
  for (const decision of [high, low, missing]) {
    assert.equal(decision.contractDigest, runtime.publication.contractDigest);
    assert.equal(
      decision.executableDigest,
      runtime.publication.activeExecutableDigest,
    );
  }
  assert.equal(capturedBodies.length, 3);
  for (const body of capturedBodies) {
    assert.equal(body.attributes._random, 0.125);
    assert.equal("runtimeTarget" in body, false);
    assert.equal("idempotencyKey" in body, false);
    assert.equal("fallback" in body, false);
  }

  await runtime.decision.stop();
  await assert.rejects(() => client.decide(contract.name));

  process.stdout.write(`${JSON.stringify({
    status: "passed",
    contractDigest: runtime.publication.contractDigest,
    executableDigest: runtime.publication.activeExecutableDigest,
    high: high.result,
    low: low.result,
    missing: missing.result,
  }, null, 2)}\n`);
}

async function ready(url, fetchImpl, signal) {
  const response = await fetchImpl(`${url}/health/ready`, { signal });
  const body = await response.text();
  if (!response.ok) {
    throw new Error(`service readiness returned HTTP ${response.status}: ${body}`);
  }
  const readiness = JSON.parse(body);
  if (readiness.status !== "ready") {
    throw new Error(`service readiness reported '${readiness.status}': ${body}`);
  }
  return true;
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () =>
      rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => runIntegration(lifecycle),
      lifecycle,
      (cleanupError) => {
        process.stderr.write(`Cleanup failure: ${formatError(cleanupError)}\n`);
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
  return error instanceof Error
    ? error.stack ?? error.message
    : String(error);
}

await main();
