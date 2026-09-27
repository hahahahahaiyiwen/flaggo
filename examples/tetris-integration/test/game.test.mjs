import assert from "node:assert/strict";
import test from "node:test";

import {
  SequencePieceSource,
  SevenBagPieceSource,
  TetrisGame,
} from "../dist/standalone/game.js";
import {
  LocalDropIntervalProvider,
  localDropInterval,
  validDropInterval,
} from "../dist/standalone/drop-interval.js";

const tetrominoes = ["I", "J", "L", "O", "S", "T", "Z"];

function moveHorizontally(game, offset) {
  const move = offset < 0
    ? () => game.moveLeft()
    : () => game.moveRight();
  for (let count = 0; count < Math.abs(offset); count += 1) {
    assert.equal(move().changed, true);
  }
}

function clearTwoLinesWithOPieces(game) {
  let finalUpdate;
  for (const offset of [-4, -2, 0, 2, 4]) {
    moveHorizontally(game, offset);
    finalUpdate = game.hardDrop();
  }
  return finalUpdate;
}

test("seven-bag source emits every tetromino once per bag", () => {
  const source = new SevenBagPieceSource(() => 0.5);
  const pieces = Array.from({ length: 14 }, () => source.next());

  assert.deepEqual([...pieces.slice(0, 7)].sort(), tetrominoes);
  assert.deepEqual([...pieces.slice(7)].sort(), tetrominoes);
});

test("seven-bag source rejects random values outside [0, 1)", () => {
  const source = new SevenBagPieceSource(() => 1);
  assert.throws(() => source.next(), RangeError);
});

test("movement respects board boundaries and records recovery context", () => {
  let clock = 100;
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["O"]),
    now: () => clock,
    sessionId: "movement-test",
  });

  moveHorizontally(game, -4);
  assert.equal(game.moveLeft().changed, false);
  clock = 650;
  assert.equal(game.hardDrop().locked, true);

  const snapshot = game.snapshot();
  assert.equal(snapshot.score, 36);
  assert.equal(snapshot.dropObservation.boardPressure, 0.1);
  assert.equal(snapshot.dropObservation.placementTimeMs, 550);
  assert.equal(snapshot.dropObservation.recoveryFailures, 1);
  assert.equal(snapshot.dropObservation.sessionId, "movement-test");
});

test("rotation changes the active shape and pause blocks actions", () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["T"]),
  });
  const before = game.snapshot().board;

  assert.equal(game.rotateClockwise().changed, true);
  assert.notDeepEqual(game.snapshot().board, before);
  game.setPaused(true);
  assert.equal(game.moveLeft().changed, false);
  assert.equal(game.tick().changed, false);
  game.setPaused(false);
  assert.equal(game.moveLeft().changed, true);
});

test("soft and hard drops award distance points", () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["O"]),
  });

  assert.equal(game.softDrop().changed, true);
  assert.equal(game.score, 1);
  assert.equal(game.hardDrop().locked, true);
  assert.equal(game.score, 35);
});

test("locking pieces clears lines and advances levels", () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["O"]),
  });

  const firstClear = clearTwoLinesWithOPieces(game);
  assert.equal(firstClear.linesCleared, 2);
  assert.equal(game.lines, 2);
  assert.equal(game.level, 0);
  assert.equal(game.score, 480);

  for (let round = 0; round < 4; round += 1) {
    clearTwoLinesWithOPieces(game);
  }
  assert.equal(game.lines, 10);
  assert.equal(game.level, 1);
});

test("stacking into the spawn area ends the game", () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["O"]),
  });

  let update;
  for (let count = 0; count < 10; count += 1) {
    update = game.hardDrop();
  }
  assert.equal(update.gameOver, true);
  assert.equal(game.gameOver, true);
  assert.equal(game.moveRight().changed, false);
});

test("local interval policy and contract bounds are deterministic", async () => {
  assert.equal(localDropInterval(0), 800);
  assert.equal(localDropInterval(4), 600);
  assert.equal(localDropInterval(20), 200);
  assert.equal(localDropInterval(99), 200);
  assert.equal(localDropInterval(-2), 800);
  assert.equal(validDropInterval(850), true);
  assert.equal(validDropInterval(825), false);

  const provider = new LocalDropIntervalProvider();
  await assert.doesNotReject(async () => {
    assert.deepEqual(
      await provider.select({
        boardPressureMean5s: 0,
        boardPressureMax5s: 0,
        currentLevel: 3,
        placementTimeMeanMs5s: 0,
        recoveryFailures5s: 0,
        piecesLocked5s: 0,
        sessionId: "local",
      }),
      {
        intervalMs: 650,
        source: "local",
        status: "local gravity policy",
      },
    );
  });
});
