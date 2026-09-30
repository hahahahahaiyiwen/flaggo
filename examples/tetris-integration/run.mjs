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
  const capturedResponses = [];
  const forwardingFetch = async (input, init) => {
    if (String(input).includes("/v3/decision-contracts/")) {
      capturedBodies.push(JSON.parse(String(init?.body)));
    }
    const response = await fetchWithAbort(input, init);
    if (String(input).includes("/decisions")) {
      capturedResponses.push(await response.clone().json());
    }
    return response;
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
    placement_time_mean_ms_5s: 1600,
    pieces_locked_5s: 2,
    session_id: "game-v3",
  };
  const highAttributes = {
    ...common,
    board_pressure_mean_5s: 0.8,
    board_pressure_max_5s: 0.9,
    recovery_failures_5s: 3,
  };
  const lowAttributes = {
    ...common,
    board_pressure_mean_5s: 0.4,
    board_pressure_max_5s: 0.5,
    recovery_failures_5s: 0,
  };
  const missingAttributes = {
    session_id: "game-v3",
  };
  const { value: high } = await client.decide(contract.name, {
    attributes: highAttributes,
  });
  const { value: low } = await client.decide(contract.name, {
    attributes: lowAttributes,
  });
  const { value: missing } = await client.decide(contract.name, {
    attributes: missingAttributes,
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
    assertNoRetiredRuntimeFields(decision);
  }
  assert.equal(capturedBodies.length, 3);
  assert.equal(capturedResponses.length, 3);
  for (const body of capturedBodies) {
    assert.deepEqual(Object.keys(body), ["attributes"]);
    assert.equal(body.attributes._random, 0.125);
    assertNoRetiredRuntimeFields(body);
  }
  for (const response of capturedResponses) {
    assertNoRetiredRuntimeFields(response);
  }

  const restHigh = await postDecisionRest({
    fetch: fetchWithAbort,
    decisionUrl: hosts.decisionUrl,
    contractName: contract.name,
    contractDigest: deployment.contractDigest,
    attributes: {
      ...highAttributes,
      _random: 0.125,
    },
    signal: lifecycle.signal,
  });
  const restMissing = await postDecisionRest({
    fetch: fetchWithAbort,
    decisionUrl: hosts.decisionUrl,
    contractName: contract.name,
    contractDigest: deployment.contractDigest,
    attributes: {
      ...missingAttributes,
      _random: 0.125,
    },
    signal: lifecycle.signal,
  });
  assertEquivalentDecision(restHigh, high);
  assertEquivalentDecision(restMissing, missing);

  await hosts.decision.stop();
  await assert.rejects(() => client.decide(contract.name));
  await hosts.contract.stop();
  await hosts.otelIngestion.stop();

  const restartedHosts = await startLocalFlaggoHosts({
    lifecycle,
    repositoryRoot,
    exampleDirectory,
    runDirectory,
  });
  const restartedHigh = await postDecisionRest({
    fetch: restartedHosts.fetch,
    decisionUrl: restartedHosts.decisionUrl,
    contractName: contract.name,
    contractDigest: deployment.contractDigest,
    attributes: {
      ...highAttributes,
      _random: 0.125,
    },
    signal: lifecycle.signal,
  });
  assertEquivalentDecision(restartedHigh, high);

  process.stdout.write(`${JSON.stringify({
    status: "passed",
    contractDigest: deployment.contractDigest,
    executableDigest: deployment.activeExecutableDigest,
    sdk: {
      high: high.result,
      low: low.result,
      missing: missing.result,
    },
    restParity: {
      high: restHigh.result,
      missing: restMissing.result,
    },
    restartPersistence: {
      high: restartedHigh.result,
      executableDigest: restartedHigh.executableDigest,
    },
    retiredRuntimeFields: "absent",
  }, null, 2)}\n`);
}

async function postDecisionRest({
  fetch,
  decisionUrl,
  contractName,
  contractDigest,
  attributes,
  signal,
}) {
  const url = `${decisionUrl}/v3/decision-contracts/${
    encodeURIComponent(contractName)
  }/versions/${encodeURIComponent(contractDigest)}/decisions`;
  const request = { attributes };
  assertNoRetiredRuntimeFields(request);
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Accept: "application/json",
      Authorization: "Flaggo-Local-Development",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(request),
    signal,
  });
  const body = await response.json();
  assert.equal(response.ok, true, JSON.stringify(body));
  assertNoRetiredRuntimeFields(body);
  return body;
}

function assertEquivalentDecision(actual, expected) {
  assert.equal(actual.result, expected.result);
  assert.deepEqual(actual.evaluation, expected.evaluation);
  assert.equal(actual.contractDigest, expected.contractDigest);
  assert.equal(actual.executableDigest, expected.executableDigest);
}

function assertNoRetiredRuntimeFields(value) {
  const retiredFields = new Set([
    "authoredExecutable",
    "confidence",
    "confirmation",
    "confirmationRequired",
    "confirmationToken",
    "contract",
    "evidence",
    "EvidenceSnapshot",
    "evidenceSnapshot",
    "exposure",
    "exposureId",
    "exposureToken",
    "fallback",
    "idempotencyKey",
    "runtimeTarget",
    "staticDefinition",
  ]);
  const pending = [value];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === null || typeof current !== "object") continue;
    if (Array.isArray(current)) {
      pending.push(...current);
      continue;
    }
    for (const [key, child] of Object.entries(current)) {
      assert.equal(
        retiredFields.has(key),
        false,
        `unexpected retired field '${key}'`,
      );
      pending.push(child);
    }
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
