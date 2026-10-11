import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { PassThrough } from "node:stream";
import test from "node:test";

import {
  LocalDropIntervalProvider,
  RollingDropIntervalContext,
} from "../dist/standalone/drop-interval.js";
import {
  SequencePieceSource,
  TetrisGame,
} from "../dist/standalone/game.js";
import {
  renderTerminal,
  runTerminalTetris,
} from "../dist/standalone/terminal.js";

function terminalStreams() {
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
  return {
    input,
    output,
    rawModes,
    rendered: () => rendered,
  };
}

async function waitFor(predicate, timeoutMs = 500) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) {
      assert.fail("Timed out waiting for terminal state.");
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

test("terminal rendering shows gameplay and provider state", async () => {
  const game = new TetrisGame({
    pieceSource: new SequencePieceSource(["I"]),
    sessionId: "render-test",
  });
  const selection = await new LocalDropIntervalProvider().select(
    new RollingDropIntervalContext(
      game.snapshot().dropObservation,
      0,
    ).snapshot(game.snapshot().dropObservation, 0),
  );

  const output = renderTerminal(game.snapshot(), selection);
  assert.match(output, /Terminal Tetris/u);
  assert.match(output, /Score: 0  Lines: 0  Level: 0/u);
  assert.match(output, /Gravity: Local: 800ms \(local gravity policy\)/u);
  assert.match(output, /Controls: arrows\/A,D move/u);
});

test("terminal session restores raw mode and cursor on quit", async () => {
  const terminal = terminalStreams();

  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
  });
  await new Promise((resolve) => setImmediate(resolve));
  terminal.input.emit("keypress", "", { name: "q" });
  await session;

  assert.deepEqual(terminal.rawModes, [true, false]);
  assert.match(terminal.rendered(), /\u001b\[\?25l/u);
  assert.match(terminal.rendered(), /\u001b\[\?25h/u);
});

test("terminal starts without waiting for the first policy response", async () => {
  const terminal = terminalStreams();
  let requestCount = 0;
  let requestAborted = false;
  const provider = {
    select(_context, signal) {
      requestCount += 1;
      signal?.addEventListener("abort", () => {
        requestAborted = true;
      }, { once: true });
      return new Promise(() => {});
    },
  };

  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    provider,
    policyRefreshIntervalMs: 10,
  });
  await waitFor(() =>
    requestCount === 1 && terminal.rendered().includes("PLAYING")
  );
  await new Promise((resolve) => setTimeout(resolve, 30));

  assert.equal(requestCount, 1);
  assert.match(terminal.rendered(), /Gravity: Local: 800ms/u);
  terminal.input.emit("keypress", "", { name: "q" });
  await session;
  assert.equal(requestAborted, true);
});

test("piece locks update the window without triggering an immediate request", async () => {
  const terminal = terminalStreams();
  const contexts = [];
  const provider = {
    async select(context) {
      contexts.push(context);
      return {
        intervalMs: 750,
        source: "flaggo",
        status: "rule low-pressure",
      };
    },
  };

  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    provider,
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "lock-window-test",
    },
    policyRefreshIntervalMs: 40,
  });
  try {
    await waitFor(() => contexts.length === 1);
    terminal.input.emit("keypress", "", { name: "space" });
    terminal.input.emit("keypress", "", { name: "space" });
    await new Promise((resolve) => setImmediate(resolve));

    assert.equal(contexts.length, 1);
    await waitFor(() => contexts.length >= 2);
    assert.equal(contexts[1].piecesLocked5s, 2);
    assert.equal(contexts[1].boardPressureMax5s, 0.2);
  } finally {
    terminal.input.emit("keypress", "", { name: "q" });
    await session;
  }
});

test("policy responses do not reset an already scheduled gravity tick", async () => {
  const terminal = terminalStreams();
  const provider = {
    async select() {
      return {
        intervalMs: 200,
        source: "flaggo",
        status: "rule accelerated",
      };
    },
  };

  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    provider,
    policyRefreshIntervalMs: 1_000,
  });
  await waitFor(() =>
    terminal.rendered().includes("Flaggo: 200ms")
  );
  await new Promise((resolve) => setTimeout(resolve, 250));

  assert.equal(
    terminal.rendered().match(/Terminal Tetris/gu)?.length,
    2,
  );
  terminal.input.emit("keypress", "", { name: "q" });
  await session;
});

test("policy refresh pauses and retains the latest successful result", async () => {
  const terminal = terminalStreams();
  let requestCount = 0;
  const provider = {
    async select() {
      requestCount += 1;
      if (requestCount === 1) {
        return {
          intervalMs: 750,
          source: "flaggo",
          status: "rule low-pressure",
        };
      }
      return {
        intervalMs: 800,
        source: "local-fallback",
        status: "Flaggo unavailable: connection refused",
      };
    },
  };

  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    provider,
    policyRefreshIntervalMs: 20,
  });
  await waitFor(() =>
    terminal.rendered().includes("Flaggo cached: 750ms")
  );
  terminal.input.emit("keypress", "", { name: "p" });
  const pausedRequestCount = requestCount;
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(requestCount, pausedRequestCount);

  terminal.input.emit("keypress", "", { name: "p" });
  await waitFor(() => requestCount > pausedRequestCount);
  terminal.input.emit("keypress", "", { name: "q" });
  await session;
});

test("adaptive terminal observes lifecycle and applies mid-piece gravity only after locking", async () => {
  const terminal = terminalStreams();
  const states = [];
  let now = 0;
  let decisions = 0;
  const session = runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    now: () => now,
    game: { pieceSource: new SequencePieceSource(["O"]) },
    fallbackIntervalMs: 800,
    policyApplication: "piece-boundary",
    policyRefreshIntervalMs: 20,
    provider: {
      async select() {
        decisions += 1;
        return {
          intervalMs: decisions === 1 ? 1_000 : 200,
          exposureId: `terminal-exposure-${decisions}`,
          source: "flaggo",
          status: "adaptive",
        };
      },
    },
    onState(state, at, transition) { states.push({ state, at, transition }); },
  });
  try {
    await waitFor(() => states.some(({ state }) => state.dropInterval.intervalMs === 1_000));
    assert.equal(states[0].state.dropInterval.intervalMs, 800);
    now = 1_000;
    terminal.input.emit("keypress", "", { name: "left" });
    await waitFor(() => states.at(-1).state.pendingDropInterval?.intervalMs === 200);
    assert.equal(states.at(-1).state.dropInterval.intervalMs, 1_000);
    const exposure = states.at(-1).state.pendingDropInterval.exposureId;
    now = 2_000;
    terminal.input.emit("keypress", "", { name: "space" });
    assert.equal(states.at(-1).state.dropInterval.intervalMs, 200);
    assert.equal(states.at(-1).state.dropInterval.exposureId, exposure);
    assert.equal(states.at(-1).transition.update.locked, true);
    terminal.input.emit("keypress", "", { name: "p" });
    assert.equal(states.at(-1).state.status, "paused");
    terminal.input.emit("keypress", "", { name: "p" });
    assert.equal(states.at(-1).state.status, "playing");
    terminal.input.emit("keypress", "", { name: "r" });
    assert.equal(states.at(-1).state.dropInterval.intervalMs, 800);
  } finally {
    terminal.input.emit("keypress", "", { name: "q" });
    await session;
  }
  assert.equal(states.at(-1).state.status, "closed");
  assert.equal(states.at(-1).at, 2_000);
  assert.deepEqual(terminal.rawModes, [true, false]);
});

test("terminal surfaces policy observation errors and restores terminal state", async () => {
  const terminal = terminalStreams();
  await assert.rejects(runTerminalTetris({
    input: terminal.input,
    output: terminal.output,
    provider: {
      async select() { return { intervalMs: 800, source: "flaggo", status: "test" }; },
    },
    onState(_state, _at, transition) {
      if (transition?.operation === "refresh-policy") throw new Error("Invalid exposure.");
    },
  }), /Invalid exposure/u);
  assert.deepEqual(terminal.rawModes, [true, false]);
  assert.match(terminal.rendered(), /\u001b\[\?25h/u);
});

for (const failure of ["startup", "command", "close"]) {
  test(`terminal restores state when ${failure} observation fails`, async () => {
    const terminal = terminalStreams();
    const session = runTerminalTetris({
      input: terminal.input,
      output: terminal.output,
      onState(state, _at, transition) {
        if (
          (failure === "startup" && transition === undefined && state.status === "playing")
          || (failure === "command" && transition?.operation === "hard-drop")
          || (failure === "close" && state.status === "closed")
        ) throw new Error(`${failure} observation failed`);
      },
    });
    const rejection = assert.rejects(session, new RegExp(`${failure} observation failed`, "u"));
    if (failure !== "startup") {
      await waitFor(() => terminal.rendered().includes("PLAYING"));
      terminal.input.emit("keypress", "", { name: failure === "close" ? "q" : "space" });
    }
    await rejection;
    assert.deepEqual(terminal.rawModes, [true, false]);
    assert.match(terminal.rendered(), /\u001b\[\?25h/u);
  });
}

test("standalone build has no Flaggo SDK dependency", async () => {
  for (
    const file of [
      "drop-interval.js",
      "game.js",
      "main.js",
      "session.js",
      "terminal.js",
    ]
  ) {
    const source = await readFile(
      new URL(`../dist/standalone/${file}`, import.meta.url),
      "utf8",
    );
    assert.doesNotMatch(source, /@flaggo\/sdk|flaggo-provider/u);
  }
});
