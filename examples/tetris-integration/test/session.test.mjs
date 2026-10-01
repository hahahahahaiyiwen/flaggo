import assert from "node:assert/strict";
import test from "node:test";

import { SequencePieceSource } from "../dist/standalone/game.js";
import { TetrisSession } from "../dist/standalone/session.js";

test("headless session exposes deterministic commands and immutable snapshots", () => {
  let clock = 100;
  const session = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "headless-session",
    },
    now: () => clock,
  });

  const initial = session.snapshot();
  assert.equal(initial.revision, 0);
  assert.equal(initial.status, "playing");
  assert.equal(initial.sessionId, "headless-session");
  initial.board[0][0] = "I";
  assert.equal(session.snapshot().board[0][0], null);

  assert.equal(session.dispatch("move-left").changed, true);
  assert.equal(session.dispatch("move-right").changed, true);
  assert.equal(session.dispatch("rotate-clockwise").changed, true);
  assert.equal(session.dispatch("soft-drop").changed, true);
  assert.equal(session.dispatch("gravity-tick").changed, true);
  assert.equal(session.snapshot().revision, 5);

  const paused = session.dispatch("pause");
  assert.equal(paused.changed, true);
  assert.equal(paused.state.status, "paused");
  const blocked = session.dispatch("move-left");
  assert.equal(blocked.changed, false);
  assert.equal(blocked.state.revision, paused.state.revision);
  assert.equal(session.dispatch("resume").state.status, "playing");

  clock = 650;
  const locked = session.dispatch("hard-drop");
  assert.equal(locked.update?.locked, true);
  assert.equal(locked.state.dropObservation.placementTimeMs, 550);

  const restarted = session.dispatch("restart");
  assert.equal(restarted.state.score, 0);
  assert.equal(restarted.state.lines, 0);
  assert.equal(restarted.state.status, "playing");
  assert.equal(
    restarted.state.board.flat().filter((cell) => cell !== null).length,
    4,
  );

  session.close();
  assert.equal(session.snapshot().status, "closed");
  assert.throws(() => session.dispatch("move-right"), /session is closed/u);
});

test("rejected recovery actions advance revision and remain observable", () => {
  const session = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "recovery-revision",
    },
  });

  for (let count = 0; count < 4; count += 1) {
    session.dispatch("move-left");
  }
  const before = session.snapshot();
  const rejected = session.dispatch("move-left");

  assert.equal(rejected.changed, true);
  assert.equal(rejected.update?.changed, false);
  assert.equal(rejected.update?.recoveryFailureRecorded, true);
  assert.equal(rejected.state.revision, before.revision + 1);
  assert.equal(rejected.state.pendingRecoveryFailures, 1);

  const locked = session.dispatch("hard-drop");
  assert.equal(locked.state.pendingRecoveryFailures, 0);
  assert.equal(locked.state.dropObservation.recoveryFailures, 1);
});

test("policy refresh derives context from real game observations", async () => {
  let clock = 0;
  const contexts = [];
  const session = new TetrisSession({
    provider: {
      async select(context) {
        contexts.push(context);
        return {
          intervalMs: 750,
          source: "flaggo",
          status: "rule low-pressure",
        };
      },
    },
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "policy-context",
    },
    now: () => clock,
  });

  clock = 1_000;
  session.dispatch("hard-drop");
  clock = 2_000;
  session.dispatch("hard-drop");
  clock = 2_500;
  const refreshed = await session.refreshPolicy();

  assert.equal(contexts.length, 1);
  assert.equal(contexts[0].sessionId, "policy-context");
  assert.equal(contexts[0].piecesLocked5s, 2);
  assert.equal(contexts[0].boardPressureMax5s, 0.2);
  assert.deepEqual(refreshed.policyContext, contexts[0]);
  assert.equal(refreshed.state.dropInterval.intervalMs, 750);
  assert.equal(refreshed.state.dropInterval.source, "flaggo");
});

test("session enforces one policy request and cancels it on pause", async () => {
  let requestCount = 0;
  let requestSignal;
  let resolveRequest;
  const session = new TetrisSession({
    provider: {
      select(_context, signal) {
        requestCount += 1;
        requestSignal = signal;
        return new Promise((resolve) => {
          resolveRequest = resolve;
        });
      },
    },
  });

  const first = session.refreshPolicy();
  const duplicate = await session.refreshPolicy();
  assert.equal(requestCount, 1);
  assert.equal(duplicate.reason, "in-flight");

  session.dispatch("pause");
  assert.equal(requestSignal.aborted, true);
  resolveRequest({
    intervalMs: 750,
    source: "flaggo",
    status: "late response",
  });
  const cancelled = await first;
  assert.equal(cancelled.reason, "cancelled");
  assert.equal(cancelled.state.status, "paused");
  assert.equal(cancelled.state.dropInterval.source, "local");
});

test("aborted policy refresh completes without provider cooperation", async () => {
  let clock = 100;
  let requestCount = 0;
  const session = new TetrisSession({
    provider: {
      select() {
        requestCount += 1;
        return new Promise(() => {});
      },
    },
    now: () => clock,
  });

  const preAborted = new AbortController();
  preAborted.abort();
  clock = 200;
  const skipped = await session.refreshPolicy(preAborted.signal);
  assert.equal(skipped.reason, "cancelled");
  assert.equal(requestCount, 0);

  clock = 150;
  assert.doesNotThrow(() => session.dispatch("move-left"));
  const pending = session.refreshPolicy();
  await Promise.resolve();
  assert.equal(requestCount, 1);
  assert.equal(session.cancelPolicyRefresh(), true);
  const cancelled = await pending;
  assert.equal(cancelled.reason, "cancelled");

  const next = session.refreshPolicy();
  await Promise.resolve();
  assert.equal(requestCount, 2);
  session.cancelPolicyRefresh();
  assert.equal((await next).reason, "cancelled");
});

test("session retains the latest Flaggo interval after refresh failure", async () => {
  let requestCount = 0;
  const session = new TetrisSession({
    provider: {
      async select() {
        requestCount += 1;
        if (requestCount === 1) {
          return {
            intervalMs: 750,
            source: "flaggo",
            status: "rule low-pressure",
          };
        }
        throw new Error("connection refused");
      },
    },
  });

  assert.equal((await session.refreshPolicy()).state.dropInterval.source, "flaggo");
  const fallback = await session.refreshPolicy();
  assert.deepEqual(fallback.state.dropInterval, {
    intervalMs: 750,
    source: "flaggo-cached",
    status: "retained after refresh failure: provider error: connection refused",
  });

  const unavailable = new TetrisSession({
    provider: {
      select() {
        throw new Error("offline");
      },
    },
  });
  assert.deepEqual((await unavailable.refreshPolicy()).state.dropInterval, {
    intervalMs: 800,
    source: "local-fallback",
    status: "provider error: offline",
  });
});

test("late local fallback uses the current game level", async () => {
  let resolveRequest;
  const session = new TetrisSession({
    provider: {
      select() {
        return new Promise((resolve) => {
          resolveRequest = resolve;
        });
      },
    },
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "current-level-fallback",
    },
  });

  const pending = session.refreshPolicy();
  await Promise.resolve();
  for (let round = 0; round < 5; round += 1) {
    clearTwoLinesWithOPieces(session);
  }
  assert.equal(session.snapshot().level, 1);
  assert.equal(session.snapshot().dropInterval.intervalMs, 750);

  resolveRequest({
    intervalMs: 800,
    source: "local-fallback",
    status: "stale level-zero fallback",
  });
  const refreshed = await pending;
  assert.equal(refreshed.state.dropInterval.intervalMs, 750);
  assert.equal(refreshed.state.dropInterval.source, "local-fallback");
});

test("multiple sessions isolate board, policy, and lifecycle state", async () => {
  const observedSessions = [];
  const provider = {
    async select(context) {
      observedSessions.push(context.sessionId);
      return {
        intervalMs: context.sessionId === "session-a" ? 700 : 850,
        source: "flaggo",
        status: "session-specific",
      };
    },
  };
  const first = new TetrisSession({
    provider,
    game: {
      pieceSource: new SequencePieceSource(["I"]),
      sessionId: "session-a",
    },
  });
  const second = new TetrisSession({
    provider,
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "session-b",
    },
  });

  first.dispatch("move-left");
  first.dispatch("hard-drop");
  second.dispatch("move-right");
  const [firstPolicy, secondPolicy] = await Promise.all([
    first.refreshPolicy(),
    second.refreshPolicy(),
  ]);

  assert.deepEqual(observedSessions.sort(), ["session-a", "session-b"]);
  assert.equal(firstPolicy.state.dropInterval.intervalMs, 700);
  assert.equal(secondPolicy.state.dropInterval.intervalMs, 850);
  assert.notDeepEqual(first.snapshot().board, second.snapshot().board);

  first.dispatch("pause");
  assert.equal(first.snapshot().status, "paused");
  assert.equal(second.snapshot().status, "playing");
  first.close();
  assert.equal(first.snapshot().status, "closed");
  assert.equal(second.snapshot().status, "playing");
});

test("concurrently created sessions receive distinct default identities", () => {
  const first = new TetrisSession({ now: () => 1_000 });
  const second = new TetrisSession({ now: () => 1_000 });

  assert.notEqual(first.snapshot().sessionId, second.snapshot().sessionId);
  const firstId = first.snapshot().sessionId;
  assert.equal(first.dispatch("restart").state.sessionId, firstId);
});

test("game-over sessions reject gameplay until restart", async () => {
  const session = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "game-over-session",
    },
  });

  for (let count = 0; count < 10; count += 1) {
    session.dispatch("hard-drop");
  }
  assert.equal(session.snapshot().status, "game-over");
  assert.equal(session.dispatch("move-left").changed, false);
  assert.equal((await session.refreshPolicy()).reason, "inactive");

  const restarted = session.dispatch("restart");
  assert.equal(restarted.state.status, "playing");
  assert.equal(restarted.state.score, 0);
});

test("session rejects nonchronological time before mutating game state", () => {
  let clock = 100;
  const session = new TetrisSession({ now: () => clock });
  const before = session.snapshot();

  clock = 99;
  assert.throws(
    () => session.dispatch("gravity-tick"),
    /timestamps must be chronological/u,
  );
  assert.deepEqual(session.snapshot(), before);
});

test("session uses one validated start timestamp", () => {
  const samples = [Number.NaN, 0];
  assert.throws(
    () => new TetrisSession({ now: () => samples.shift() }),
    /session timestamps must be finite/u,
  );

  let clock = 100;
  const session = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "single-start-time",
    },
    now: () => clock,
  });
  clock = 650;
  assert.equal(
    session.dispatch("hard-drop").state.dropObservation.placementTimeMs,
    550,
  );
});

function moveHorizontally(session, offset) {
  const command = offset < 0 ? "move-left" : "move-right";
  for (let count = 0; count < Math.abs(offset); count += 1) {
    assert.equal(session.dispatch(command).update?.changed, true);
  }
}

function clearTwoLinesWithOPieces(session) {
  let finalTransition;
  for (const offset of [-4, -2, 0, 2, 4]) {
    moveHorizontally(session, offset);
    finalTransition = session.dispatch("hard-drop");
  }
  assert.equal(finalTransition.update?.linesCleared, 2);
}
