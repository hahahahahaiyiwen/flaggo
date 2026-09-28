import assert from "node:assert/strict";
import test from "node:test";

import {
  dropIntervalObservationWindowMs,
  dropIntervalRefreshIntervalMs,
  RollingDropIntervalContext,
} from "../dist/standalone/drop-interval.js";

function observation(overrides = {}) {
  return {
    boardPressure: 0.2,
    currentLevel: 3,
    placementTimeMs: 0,
    recoveryFailures: 0,
    sessionId: "rolling-test",
    ...overrides,
  };
}

test("decision observation and refresh cadence are five seconds", () => {
  assert.equal(dropIntervalObservationWindowMs, 5_000);
  assert.equal(dropIntervalRefreshIntervalMs, 5_000);
});

test("rolling context summarizes the trailing five seconds", () => {
  const window = new RollingDropIntervalContext(observation(), 0);
  window.record(observation({
    boardPressure: 0.8,
    placementTimeMs: 1_000,
    recoveryFailures: 1,
  }), 2_000, true);
  window.record(observation({
    boardPressure: 0.4,
    placementTimeMs: 500,
    recoveryFailures: 2,
  }), 4_000, true);

  assert.deepEqual(window.snapshot(observation({
    boardPressure: 0.4,
  }), 5_000), {
    boardPressureMean5s: 0.48,
    boardPressureMax5s: 0.8,
    currentLevel: 3,
    placementTimeMeanMs5s: 750,
    recoveryFailures5s: 3,
    piecesLocked5s: 2,
    sessionId: "rolling-test",
  });
});

test("rolling context expires observations outside the window", () => {
  const window = new RollingDropIntervalContext(observation(), 0);
  window.record(observation({
    boardPressure: 0.8,
    placementTimeMs: 1_000,
    recoveryFailures: 1,
  }), 2_000, true);
  window.record(observation({
    boardPressure: 0.4,
    placementTimeMs: 500,
    recoveryFailures: 2,
  }), 4_000, true);

  assert.deepEqual(window.snapshot(observation({
    boardPressure: 0.4,
  }), 8_000), {
    boardPressureMean5s: 0.48,
    boardPressureMax5s: 0.8,
    currentLevel: 3,
    placementTimeMeanMs5s: 500,
    recoveryFailures5s: 2,
    piecesLocked5s: 1,
    sessionId: "rolling-test",
  });
});

test("rolling context rejects cross-session and nonchronological samples", () => {
  const window = new RollingDropIntervalContext(observation(), 100);

  assert.throws(
    () => window.record(observation({ sessionId: "other" }), 101),
    /cannot span game sessions/u,
  );
  assert.throws(
    () => window.record(observation(), 99),
    /must be chronological/u,
  );
});
