import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { PassThrough } from "node:stream";
import test from "node:test";

import {
  LocalDropIntervalProvider,
} from "../dist/standalone/drop-interval.js";
import {
  SequencePieceSource,
  TetrisGame,
} from "../dist/standalone/game.js";
import {
  renderTerminal,
  runTerminalTetris,
} from "../dist/standalone/terminal.js";

test("terminal rendering shows gameplay and provider state", async () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["I"]),
    sessionId: "render-test",
  });
  const selection = await new LocalDropIntervalProvider().select(
    game.snapshot().dropContext,
  );

  const output = renderTerminal(game.snapshot(), selection);
  assert.match(output, /Terminal Tetris/u);
  assert.match(output, /Score: 0  Lines: 0  Level: 0/u);
  assert.match(output, /Gravity: Local: 800ms \(local gravity policy\)/u);
  assert.match(output, /Controls: arrows\/A,D move/u);
});

test("terminal session restores raw mode and cursor on quit", async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  const rawModes = [];
  let rendered = "";
  Object.defineProperty(input, "isTTY", { value: true });
  Object.defineProperty(output, "isTTY", { value: true });
  input.setRawMode = (enabled) => {
    rawModes.push(enabled);
    return input;
  };
  output.on("data", (chunk) => {
    rendered += chunk.toString();
  });

  const session = runTerminalTetris({ input, output });
  await new Promise((resolve) => setImmediate(resolve));
  input.emit("keypress", "", { name: "q" });
  await session;

  assert.deepEqual(rawModes, [true, false]);
  assert.match(rendered, /\u001b\[\?25l/u);
  assert.match(rendered, /\u001b\[\?25h/u);
});

test("standalone build has no Flaggo SDK dependency", async () => {
  for (const file of ["drop-interval.js", "game.js", "main.js", "terminal.js"]) {
    const source = await readFile(
      new URL(`../dist/standalone/${file}`, import.meta.url),
      "utf8",
    );
    assert.doesNotMatch(source, /@flaggo\/sdk|flaggo-provider/u);
  }
});
