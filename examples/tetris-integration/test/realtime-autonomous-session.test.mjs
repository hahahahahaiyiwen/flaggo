import assert from "node:assert/strict";
import test from "node:test";
import { SequencePieceSource } from "../dist/flaggo/game.js";
import { scenarioPieceSource } from "../dist/flaggo/adaptive-protocol.js";
import { runRealtimeAutonomousTetrisSession } from "../dist/flaggo/realtime-autonomous-session.js";

function harness(context, options = {}) {
  let clock = 0;
  const states = [];
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const task = runRealtimeAutonomousTetrisSession({
    durationMs: 5_000,
    reactionDelayMs: 3_000,
    commandIntervalMs: 200,
    pieceSource: new SequencePieceSource(["O"]),
    now: () => clock,
    onState(state, at, transition) { states.push({ state, at, transition }); },
    ...options,
  });
  return {
    task, states,
    async advance(to) {
      while (clock < to) {
        const next = Math.min(to, clock + 50);
        const delta = next - clock;
        clock = next;
        context.mock.timers.tick(delta);
        for (let turn = 0; turn < 20; turn += 1) await Promise.resolve();
      }
    },
  };
}

test("real-time physics continues during a request and queues a mid-piece response", async (context) => {
  let complete;
  const h = harness(context, {
    provider: { select() { return new Promise((resolve) => { complete = resolve; }); } },
  });
  await h.advance(950);
  assert.ok(h.states.some((value) => value.transition?.operation === "gravity-tick"));
  assert.equal(h.states.at(-1).state.dropInterval.intervalMs, 800);
  complete({ intervalMs: 200, source: "flaggo", status: "test" });
  await h.advance(1_000);
  assert.equal(h.states.at(-1).state.pendingDropInterval.intervalMs, 200);
  assert.equal(h.states.at(-1).state.dropInterval.intervalMs, 800);
  await h.advance(5_000);
  const result = await h.task;
  assert.ok(result.piecesLocked > 0);
  assert.ok(result.hardDrops > 0);
  assert.ok(h.states.some((value) =>
    value.transition?.update?.locked && value.state.dropInterval.intervalMs === 200));
  assert.equal(result.elapsedMs, 5_000);
  assert.equal(h.states.at(-1).state.status, "closed");
});

test("gravity wins scheduled ties without replacing independent command pacing", async (context) => {
  const h = harness(context, { reactionDelayMs: 800, commandIntervalMs: 200 });
  await h.advance(5_000);
  await h.task;
  const operations = h.states.filter((value) => value.transition && value.at === 800);
  assert.equal(operations.length, 2);
  assert.equal(operations[0].transition.operation, "gravity-tick");
  assert.notEqual(operations[1].transition.operation, "gravity-tick");
  assert.ok(h.states.some((value) =>
    value.at === 1_000 && value.transition?.operation === "move-left"));
});

test("cancellation closes even when an external provider ignores abort", async (context) => {
  const controller = new AbortController();
  const h = harness(context, {
    signal: controller.signal,
    provider: { select() { return new Promise(() => {}); } },
  });
  const rejection = assert.rejects(h.task, /stopped/u);
  await h.advance(1_000);
  controller.abort(new Error("stopped"));
  await h.advance(1_050);
  await rejection;
  assert.equal(h.states.at(-1).state.status, "closed");
});

test("a late response does not delay gravity after a previously queued rate applies", async (context) => {
  const replies = [];
  const h = harness(context, {
    durationMs: 10_000,
    reactionDelayMs: 6_000,
    provider: {
      select() { return new Promise((resolve) => replies.push(resolve)); },
    },
  });
  await h.advance(1_000);
  replies[0]({ intervalMs: 200, source: "flaggo", status: "test" });
  await h.advance(5_000);
  assert.equal(replies.length, 2);
  let locked;
  for (let at = 5_050; at < 9_000 && locked === undefined; at += 50) {
    await h.advance(at);
    locked = h.states.find((value) => value.transition?.update?.locked);
  }
  assert.ok(locked);
  assert.equal(locked.state.dropInterval.intervalMs, 200);
  await h.advance(locked.at + 100);
  replies[1]({ intervalMs: 200, source: "flaggo", status: "test" });
  await h.advance(locked.at + 200);
  assert.ok(h.states.some((value) =>
    value.at === locked.at + 200 && value.transition?.operation === "gravity-tick"));
  await h.advance(10_000);
  await h.task;
});

test("observer failure is surfaced and closes the episode", async (context) => {
  const states = [];
  const h = harness(context, {
    onState(state) {
      states.push(state);
      if (state.status !== "closed") throw new Error("observer failed");
    },
  });
  await assert.rejects(h.task, /observer failed/u);
  assert.equal(states.at(-1).status, "closed");
});

test("real-time gravity changes outcomes with unchanged greedy strategy and pacing", async (context) => {
  const run = async (dropIntervalMs) => {
    const h = harness(context, {
      durationMs: 120_000,
      dropIntervalMs,
      pieceSource: scenarioPieceSource(11),
    });
    await h.advance(120_000);
    const result = await h.task;
    context.mock.timers.reset();
    return result;
  };
  const fast = await run(200);
  const slow = await run(1_000);
  assert.equal(fast.state.gameOver, true);
  assert.equal(slow.state.gameOver, false);
  assert.ok(slow.state.lines > fast.state.lines);
  assert.ok(slow.elapsedMs > fast.elapsedMs);
  assert.equal(slow.elapsedMs, 120_000);
});
