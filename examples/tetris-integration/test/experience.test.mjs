import assert from "node:assert/strict";
import test from "node:test";

import { SequencePieceSource } from "../dist/flaggo/game.js";
import { TetrisSession } from "../dist/flaggo/session.js";
import { createExperienceRecorder, TetrisExperience } from "../dist/flaggo/experience.js";

function harness(onWindow) {
  let clock = 0;
  let interval = 800;
  const windows = [];
  const inputs = [];
  const experience = new TetrisExperience(
    { dataset: "training", origin: "greedy", clock: "virtual" }, (window) => {
      windows.push(window);
      onWindow?.(window);
    });
  const session = new TetrisSession({
    game: { pieceSource: new SequencePieceSource(["O"]), sessionId: "experience-test" },
    now: () => clock,
    fallbackIntervalMs: 800,
    policyApplication: "piece-boundary",
    provider: {
      async select(context) {
        const input = experience.runtimeInput(context);
        inputs.push(input);
        experience.registerDecision(input, interval);
        return { intervalMs: interval, source: "flaggo", status: "test", exposureId: input.exposure_id };
      },
    },
  });
  experience.observe(session.snapshot(), clock);
  return {
    session, experience, windows, inputs,
    at(value) { clock = value; },
    interval(value) { interval = value; },
    command(command) {
      const transition = session.dispatch(command);
      experience.observe(transition.state, clock, transition);
      return transition;
    },
    async refresh() {
      const transition = await session.refreshPolicy();
      experience.observe(transition.state, clock, transition);
      return transition;
    },
    observe() { experience.observe(session.snapshot(), clock); },
    close() {
      session.close();
      experience.observe(session.snapshot(), clock);
    },
  };
}

test("startup input uses the actual interval and qualified empty observations", async () => {
  const h = harness();
  await h.refresh();
  const input = h.inputs[0];
  assert.equal(Object.keys(input).length, 12);
  assert.equal(input.current_drop_interval_ms, 800);
  assert.equal(input.pieces_locked_5s, 0);
  assert.equal(input.active_play_ms_5s, 0);
  assert.equal(input.hard_drop_ratio_5s, 0);
  assert.equal(input.challenge_balance_5s, 0);
  assert.equal(h.windows.length, 0);
  h.close();
});

test("locking outcomes stay on the old exposure when a queued decision applies", async () => {
  const h = harness();
  await h.refresh();
  const oldExposure = h.inputs[0].exposure_id;
  for (let column = 0; column < 4; column += 1) {
    h.at(500 * (column + 1));
    const moves = column * 2 - 4;
    for (let move = 0; move < Math.abs(moves); move += 1) {
      h.command(moves < 0 ? "move-left" : "move-right");
    }
    h.command("hard-drop");
  }
  h.at(2_300);
  for (let move = 0; move < 4; move += 1) h.command("move-right");
  h.interval(200);
  const queued = await h.refresh();
  const newExposure = h.inputs[1].exposure_id;
  assert.equal(queued.reason, "queued");
  assert.equal(queued.state.dropInterval.intervalMs, 800);
  assert.equal(queued.state.pendingDropInterval.intervalMs, 200);
  assert.equal(h.inputs[1].current_drop_interval_ms, 800);
  assert.equal(h.inputs[1].hard_drop_ratio_5s, 1);
  assert.equal(h.inputs[1].active_play_ms_5s, 2_300);
  assert.equal(h.windows.length, 0);

  h.at(2_500);
  const locked = h.command("hard-drop");
  assert.equal(locked.update.linesCleared, 2);
  assert.equal(locked.state.dropInterval.exposureId, newExposure);
  assert.equal(locked.state.pendingDropInterval, undefined);
  assert.equal(h.windows[0].input.exposure_id, oldExposure);
  assert.equal(h.windows[0].linesCleared, 2);
  assert.equal(h.windows[0].activePlayMs, 2_500);
  assert.equal(h.windows[0].startAtMs, 0);
  assert.equal(h.windows[0].endAtMs, 2_500);
  assert.ok(Object.isFrozen(h.windows[0].input));
  h.at(3_000);
  h.command("pause");
  assert.equal(h.windows[1].input.exposure_id, newExposure);
  assert.equal(h.windows[1].activePlayMs, 500);
  assert.equal(h.windows[1].linesCleared, 0);
  assert.equal(h.experience.totals().linesCleared, 2);
  h.close();
});

test("cancelled queued decisions receive no applied outcomes", async () => {
  const h = harness();
  await h.refresh();
  h.at(1_000);
  h.command("move-left");
  h.interval(200);
  await h.refresh();
  const discarded = h.inputs[1].exposure_id;
  h.command("pause");
  assert.equal(h.session.snapshot().pendingDropInterval, undefined);
  h.at(10_000);
  h.command("resume");
  h.at(11_000);
  h.command("hard-drop");
  h.close();
  assert.ok(h.windows.every((window) => window.input.exposure_id !== discarded));
  assert.equal(h.experience.totals().activePlayMs, 2_000);
});

test("long observations split into bounded nonoverlapping active windows", async () => {
  const h = harness();
  await h.refresh();
  h.at(12_250);
  h.command("pause");
  assert.deepEqual(h.windows.map((w) => w.activePlayMs), [5_000, 5_000, 2_250]);
  assert.deepEqual(h.windows.map((w) => [w.startAtMs, w.endAtMs]),
    [[0, 5_000], [5_000, 10_000], [10_000, 12_250]]);
  assert.equal(new Set(h.windows.map((w) => w.windowId)).size, 3);
  h.at(30_000);
  h.observe();
  assert.equal(h.experience.totals().activePlayMs, 12_250);
  h.close();
});

test("pressure balance is integrated from settled board state, not a chosen interval", async () => {
  const h = harness();
  await h.refresh();
  h.at(1_000);
  h.command("hard-drop");
  h.at(2_000);
  h.command("hard-drop");
  h.at(4_000);
  h.command("pause");
  assert.equal(h.windows[0].activePlayMs, 4_000);
  assert.equal(h.windows[0].challengeBalance, 0.5);
  assert.equal(h.experience.totals().balancedPlayMs, 2_000);
  h.close();
});

test("reporting failure does not count the same active time again during close", async () => {
  const h = harness(() => { throw new Error("reporter failed"); });
  await h.refresh();
  h.at(5_250);
  assert.throws(() => h.command("pause"), /reporter failed/u);
  h.close();
  assert.equal(h.experience.totals().activePlayMs, 5_250);
  assert.equal(h.windows.length, 1);
});

test("game over is counted once and completion is an explicit zero-duration fact", async () => {
  const h = harness();
  await h.refresh();
  for (let piece = 1; !h.session.snapshot().gameOver; piece += 1) {
    h.at(piece * 1_000);
    h.command("hard-drop");
  }
  h.observe();
  h.close();
  assert.equal(h.experience.totals().gameOvers, 1);
  assert.equal(h.windows.reduce((count, w) => count + w.gameOvers, 0), 1);
  assert.ok(h.windows.every((w) => !w.episodeComplete));
  h.experience.completeEpisode();
  h.experience.completeEpisode();
  const complete = h.windows.filter((w) => w.episodeComplete);
  assert.equal(complete.length, 1);
  assert.equal(complete[0].activePlayMs, 0);
  assert.equal(complete[0].gameOvers, 0);
});

test("experience refuses cross-session, unregistered, and nonchronological state", async () => {
  const h = harness();
  await h.refresh();
  assert.throws(() => h.experience.observe(h.session.snapshot(), -1), /chronological/u);
  assert.throws(() => h.experience.observe(h.session.snapshot(), NaN), /finite/u);
  assert.throws(() => h.experience.observe({ ...h.session.snapshot(), sessionId: "other" }, 0),
    /one Tetris session/u);
  assert.throws(() => h.experience.registerDecision(h.inputs[0], 800), /not be reused/u);
  assert.throws(() => h.experience.registerDecision({ ...h.inputs[0], exposure_id: "bad" }, 825),
    /bounded decision/u);
  h.close();
});

test("experience gauges carry unique windows, input context, and exact exposure correlation", () => {
  const points = [];
  const record = createExperienceRecorder({
    createGauge(name, options) {
      return { record(value, attributes) { points.push({ name, unit: options.unit, value, attributes }); } };
    },
  });
  record({
    windowId: "window-1", input: {
      session_id: "session-1", exposure_id: "exposure-1",
      board_pressure_mean_5s: 0.1, hard_drop_ratio_5s: 1, pieces_locked_5s: 2,
      active_play_ms_5s: 5_000,
    },
    dataset: "training", origin: "greedy", clock: "virtual",
    intervalMs: 200, startAtMs: 0, endAtMs: 5_000,
    activePlayMs: 5_000, challengeBalance: 0.8, linesCleared: 2,
    gameOvers: 0, episodeComplete: false,
  });
  assert.deepEqual(points.map((p) => [p.name, p.unit, p.value]), [
    ["tetris.experience.challenge_balance", "1", 0.8],
    ["tetris.experience.lines_cleared", "{line}", 2],
    ["tetris.experience.game_over_count", "{game}", 0],
    ["tetris.experience.active_play_ms", "ms", 5_000],
  ]);
  for (const point of points) {
    assert.equal(point.attributes["tetris.session.id"], "session-1");
    assert.equal(point.attributes["tetris.exposure.id"], "exposure-1");
    assert.equal(point.attributes["tetris.experience.window.id"], "window-1");
    assert.equal(point.attributes["tetris.input.board_pressure_mean_5s"], 0.1);
    assert.equal(point.attributes["tetris.experience.clock"], "virtual");
    assert.equal(point.attributes["tetris.player.origin"], "greedy");
    assert.equal(point.attributes["tetris.input.active_play_ms_5s"], 5_000);
  }
  points.length = 0;
  record({
    windowId: "live-window", input: {
      session_id: "human-session", exposure_id: "human-exposure",
      board_pressure_mean_5s: 0, hard_drop_ratio_5s: 0, pieces_locked_5s: 0,
      active_play_ms_5s: 0,
    },
    dataset: "live", origin: "human", clock: "wall",
    intervalMs: 800, startAtMs: 0, endAtMs: 1_000,
    activePlayMs: 1_000, challengeBalance: 0, linesCleared: 0,
    gameOvers: 0, episodeComplete: false,
  });
  for (const point of points) {
    assert.equal(point.attributes["tetris.experience.dataset"], "live");
    assert.equal(point.attributes["tetris.experience.clock"], "wall");
    assert.equal(point.attributes["tetris.player.origin"], "human");
  }
});
