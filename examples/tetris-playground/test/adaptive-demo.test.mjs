import assert from "node:assert/strict";
import test from "node:test";

import {
  adaptiveDemoProtocol,
  scenarioPieceSource,
} from "../../tetris-integration/dist/flaggo/adaptive-protocol.js";
import { assertExperienceImprovement } from "../dist/adaptive-demo.js";

const baseline = {
  executableDigest: "authored", episodes: 3, plannedPlayMs: 360_000,
  activePlayMs: 180_000, challengeBalance: 0.5, linesCleared: 6,
  linesPerMinute: 2, gameOvers: 3,
};
const learned = {
  ...baseline, executableDigest: "learned", activePlayMs: 360_000,
  challengeBalance: 0.8, linesCleared: 18, linesPerMinute: 3, gameOvers: 0,
};

test("matched scenarios reproduce pieces without overlap with training", () => {
  assert.equal(adaptiveDemoProtocol.trainingSeeds.length, 9);
  assert.equal(adaptiveDemoProtocol.evaluationSeeds.length, 3);
  for (const seed of adaptiveDemoProtocol.evaluationSeeds) {
    assert.ok(!adaptiveDemoProtocol.trainingSeeds.includes(seed));
    const first = scenarioPieceSource(seed);
    const second = scenarioPieceSource(seed);
    for (let piece = 0; piece < 100; piece += 1) assert.equal(first.next(), second.next());
  }
});

test("measured completion requires the actual improvement threshold", () => {
  assert.doesNotThrow(() => assertExperienceImprovement(baseline, learned));
  assert.throws(() => assertExperienceImprovement(baseline,
    { ...learned, challengeBalance: 0.549 }), /at least 5/u);
  assert.doesNotThrow(() => assertExperienceImprovement(baseline,
    { ...learned, challengeBalance: 0.55 }));
  assert.doesNotThrow(() => assertExperienceImprovement({ ...baseline, challengeBalance: 0.9 },
    { ...learned, challengeBalance: 0.95 }));
});

test("invalid measurements cannot look like a successful comparison", () => {
  for (const name of ["challengeBalance", "linesPerMinute", "activePlayMs", "gameOvers"]) {
    assert.throws(() => assertExperienceImprovement(baseline,
      { ...learned, [name]: NaN }), /finite gameplay/u);
  }
  assert.throws(() => assertExperienceImprovement(baseline,
    { ...learned, activePlayMs: 360_001 }), /bounded/u);
});

test("throughput retention and survival guardrails are independent of balance", () => {
  assert.throws(() => assertExperienceImprovement(baseline,
    { ...learned, linesPerMinute: 1.799 }), /90%/u);
  assert.doesNotThrow(() => assertExperienceImprovement(baseline,
    { ...learned, linesPerMinute: 1.8 }));
  assert.throws(() => assertExperienceImprovement({ ...baseline, gameOvers: 0 },
    { ...learned, gameOvers: 1 }), /game overs/u);
  assert.throws(() => assertExperienceImprovement(baseline,
    { ...learned, activePlayMs: 179_999 }), /earlier/u);
  assert.throws(() => assertExperienceImprovement(baseline,
    { ...learned, plannedPlayMs: 480_000 }), /same episode budget/u);
});
