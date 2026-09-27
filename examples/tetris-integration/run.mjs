import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createDecisionClient,
} from "@flaggo/sdk/runtime";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
} from "./host-process.mjs";
import { deployTetrisContract } from "./deploy-contract.mjs";
import { startLocalFlaggoHosts } from "./local-flaggo.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `tetris-integration-${process.pid}-${Date.now()}`,
);

async function runIntegration(lifecycle) {
  const hosts = await startLocalFlaggoHosts({
    lifecycle,
    repositoryRoot,
    exampleDirectory,
    runDirectory,
  });
  const deployed = await deployTetrisContract({
    exampleDirectory,
    contractUrl: hosts.contractUrl,
    fetch: hosts.fetch,
    signal: lifecycle.signal,
  });
  const contract = deployed.contract;
  const deployment = deployed.deployment;
  const fetchWithAbort = hosts.fetch;

  const capturedBodies = [];
  const forwardingFetch = async (input, init) => {
    if (String(input).includes("/v3/decision-contracts/")) {
      capturedBodies.push(JSON.parse(String(init?.body)));
    }
    return fetchWithAbort(input, init);
  };
  const client = createDecisionClient({
    baseUrl: hosts.decisionUrl,
    bindings: {
      [contract.name]: {
        contractDigest: deployment.contractDigest,
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
  const { value: high } = await client.decide(contract.name, {
    attributes: {
      ...common,
      board_pressure: 0.9,
      recovery_failures: 3,
    },
  });
  const { value: low } = await client.decide(contract.name, {
    attributes: {
      ...common,
      board_pressure: 0.4,
      recovery_failures: 0,
    },
  });
  const { value: missing } = await client.decide(contract.name, {
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
    assert.equal(decision.contractDigest, deployment.contractDigest);
    assert.equal(
      decision.executableDigest,
      deployment.activeExecutableDigest,
    );
  }
  assert.equal(capturedBodies.length, 3);
  for (const body of capturedBodies) {
    assert.equal(body.attributes._random, 0.125);
    assert.equal("runtimeTarget" in body, false);
    assert.equal("idempotencyKey" in body, false);
    assert.equal("fallback" in body, false);
  }

  await hosts.decision.stop();
  await assert.rejects(() => client.decide(contract.name));

  process.stdout.write(`${JSON.stringify({
    status: "passed",
    contractDigest: deployment.contractDigest,
    executableDigest: deployment.activeExecutableDigest,
    high: high.result,
    low: low.result,
    missing: missing.result,
  }, null, 2)}\n`);
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
