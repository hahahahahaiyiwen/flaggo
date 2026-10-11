import assert from "node:assert/strict";
import test from "node:test";

import { runAutonomousTetrisSession } from "../dist/standalone/autonomous-session.js";
import { SequencePieceSource, SevenBagPieceSource } from "../dist/standalone/game.js";
import { chooseGreedyPlacement, GreedyTetrisPlayer } from "../dist/standalone/greedy-player.js";
import { TetrisSession } from "../dist/standalone/session.js";

test("greedy plans use legal engine commands and predict actual line clears", () => {
  const session = new TetrisSession({
    game: { pieceSource: new SequencePieceSource(["O"]) },
  });
  for (let piece = 0; piece < 50; piece += 1) {
    const before = session.planningState();
    const plan = chooseGreedyPlacement(before);
    assert.deepEqual(session.planningState(), before);
    let cleared = 0;
    for (const command of plan.commands) {
      const transition = session.dispatch(command);
      assert.equal(transition.update?.changed, true);
      assert.equal(transition.update?.recoveryFailureRecorded, false);
      cleared += transition.update?.linesCleared ?? 0;
    }
    assert.equal(cleared, plan.linesCleared);
    assert.deepEqual(session.planningState().board, plan.board);
  }
  assert.equal(session.snapshot().lines, 20);
  assert.equal(session.snapshot().gameOver, false);
  session.close();
});

test("planning snapshots cannot mutate the game", () => {
  const session = new TetrisSession();
  const planning = session.planningState();
  planning.board[19][0] = "I";
  planning.activePiece.x = -20;
  assert.equal(session.planningState().board[19][0], null);
  assert.notEqual(session.planningState().activePiece.x, -20);
  session.close();
});

test("player reaction and command cadence are independent of gravity", () => {
  let now = 0;
  const session = new TetrisSession({ now: () => now });
  const player = new GreedyTetrisPlayer(session, {
    reactionDelayMs: 500,
    commandIntervalMs: 100,
  });
  assert.equal(player.nextActionAt(now), 500);
  now = 499;
  assert.equal(player.nextCommand(now), undefined);
  now = 500;
  session.dispatch(player.nextCommand(now));
  assert.equal(player.nextActionAt(now), 600);
  session.dispatch("pause");
  assert.equal(player.nextCommand(700), undefined);
  now = 800;
  session.dispatch("resume");
  assert.equal(player.nextActionAt(now), 1_300);
  session.dispatch("restart");
  player.reset();
  assert.equal(player.nextActionAt(now), 1_300);
  assert.throws(() => player.nextActionAt(799), /chronological/u);
  session.close();
});

test("paced mixed-piece gameplay sustains real line clearing", async () => {
  const result = await runAutonomousTetrisSession({
    durationMs: 120_000,
    reactionDelayMs: 500,
    commandIntervalMs: 100,
    dropIntervalMs: 800,
    pieceSource: new SevenBagPieceSource(() => 0.5),
  });
  assert.equal(result.elapsedMs, 120_000);
  assert.equal(result.state.gameOver, false);
  assert.ok(result.piecesLocked >= 80, `${result.piecesLocked} locked pieces`);
  assert.ok(result.state.lines >= 20, `${result.state.lines} cleared lines`);
  assert.equal(result.hardDrops, result.piecesLocked);
});

test("gravity changes genuine outcomes with the same strategy and pacing", async () => {
  const run = (dropIntervalMs) => runAutonomousTetrisSession({
    durationMs: 60_000,
    reactionDelayMs: 4_000,
    commandIntervalMs: 200,
    dropIntervalMs,
    pieceSource: new SequencePieceSource(["O"]),
  });
  const [fast, slow] = await Promise.all([run(200), run(1_000)]);
  assert.equal(fast.state.gameOver, true);
  assert.equal(fast.hardDrops, 0);
  assert.ok(fast.elapsedMs < 60_000);
  assert.equal(slow.state.gameOver, false);
  assert.ok(slow.hardDrops >= 10);
  assert.ok(slow.state.lines >= 4);
});

test("concurrent sessions retain separate identities, boards, and clocks", async () => {
  const run = (sessionId, durationMs, piece) => runAutonomousTetrisSession({
    sessionId,
    durationMs,
    reactionDelayMs: 500,
    commandIntervalMs: 100,
    pieceSource: new SequencePieceSource([piece]),
  });
  const [first, second] = await Promise.all([
    run("autonomous-a", 5_000, "O"),
    run("autonomous-b", 10_000, "I"),
  ]);
  assert.equal(first.state.sessionId, "autonomous-a");
  assert.equal(second.state.sessionId, "autonomous-b");
  assert.equal(first.elapsedMs, 5_000);
  assert.equal(second.elapsedMs, 10_000);
  assert.notDeepEqual(first.state.board, second.state.board);
});

test("aborted episodes close instrumentation and stop issuing commands", async () => {
  const controller = new AbortController();
  let active = 0;
  let commands = 0;
  await assert.rejects(runAutonomousTetrisSession({
    durationMs: 10_000,
    reactionDelayMs: 500,
    commandIntervalMs: 100,
    signal: controller.signal,
    instrumentation: {
      sessionStarted() { active += 1; },
      sessionClosed() { active -= 1; },
      commandStarted() {
        commands += 1;
        return { completed() {}, failed() {} };
      },
    },
    onState(_state, now) {
      if (now >= 1_000) controller.abort(new Error("Stop episode"));
    },
  }), /Stop episode/u);
  assert.equal(active, 0);
  assert.ok(commands > 0);
});

test("invalid episode settings fail explicitly", async () => {
  const options = { durationMs: 1_000, reactionDelayMs: 500, commandIntervalMs: 100 };
  await assert.rejects(runAutonomousTetrisSession({ ...options, durationMs: 0 }), RangeError);
  await assert.rejects(runAutonomousTetrisSession({ ...options, reactionDelayMs: -1 }), RangeError);
  await assert.rejects(runAutonomousTetrisSession({ ...options, commandIntervalMs: 0 }), RangeError);
  await assert.rejects(runAutonomousTetrisSession({ ...options, dropIntervalMs: 825 }), RangeError);
});
