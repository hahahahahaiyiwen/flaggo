import assert from "node:assert/strict";
import test from "node:test";

import {
  createFlaggoDropIntervalProvider,
} from "../dist/flaggo/flaggo-provider.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";

const context = {
  boardPressure: 0.9,
  currentLevel: 8,
  recentPlacementTimeMs: 1_600,
  recoveryFailures: 3,
  sessionId: "provider-test",
};

function decisionResponse(result, evaluation = { source: "rule", rule: "high-pressure" }) {
  return new Response(JSON.stringify({
    contractDigest,
    executableDigest,
    result,
    evaluation,
  }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

test("Flaggo provider maps game context and returns decision provenance", async () => {
  let request;
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async (input, init) => {
      request = { input, init };
      return decisionResponse(850);
    },
  });

  assert.deepEqual(await provider.select(context), {
    intervalMs: 850,
    source: "flaggo",
    status: "rule high-pressure",
  });
  assert.match(
    String(request.input),
    /\/v3\/decision-contracts\/tetris\.dropInterval\/versions\//u,
  );
  const body = JSON.parse(String(request.init.body));
  assert.deepEqual(
    {
      ...body.attributes,
      _random: undefined,
    },
    {
      board_pressure: 0.9,
      current_level: 8,
      recent_placement_time_ms: 1_600,
      recovery_failures: 3,
      session_id: "provider-test",
      _random: undefined,
    },
  );
  assert.equal(typeof body.attributes._random, "number");
});

test("Flaggo provider visibly falls back to the local application policy", async () => {
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async () => {
      throw new TypeError("connection refused");
    },
  });

  const selection = await provider.select(context);
  assert.equal(selection.intervalMs, 400);
  assert.equal(selection.source, "local-fallback");
  assert.match(selection.status, /^Flaggo unavailable: /u);
  assert.match(selection.status, /could not reach the service/u);
});

test("Flaggo provider rejects unusable results through local fallback", async () => {
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async () => decisionResponse(825, { source: "default" }),
  });

  const selection = await provider.select(context);
  assert.equal(selection.intervalMs, 400);
  assert.equal(selection.source, "local-fallback");
  assert.match(selection.status, /outside the contract bounds/u);
});
