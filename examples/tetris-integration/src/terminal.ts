import { emitKeypressEvents, type Key } from "node:readline";

import {
  LocalDropIntervalProvider,
  localDropInterval,
  type DropIntervalProvider,
  type DropIntervalSelection,
} from "./drop-interval.js";
import {
  TetrisGame,
  type GameSnapshot,
  type GameUpdate,
  type TetrisGameOptions,
} from "./game.js";

export interface TerminalTetrisOptions {
  readonly provider?: DropIntervalProvider;
  readonly signal?: AbortSignal;
  readonly input?: NodeJS.ReadStream;
  readonly output?: NodeJS.WriteStream;
  readonly game?: TetrisGameOptions;
}

const clearScreen = "\u001b[2J\u001b[H";
const hideCursor = "\u001b[?25l";
const showCursor = "\u001b[?25h";

function statusLine(selection: DropIntervalSelection): string {
  const source = selection.source === "flaggo"
    ? "Flaggo"
    : selection.source === "local-fallback"
      ? "Local fallback"
      : "Local";
  return `${source}: ${selection.intervalMs}ms (${selection.status})`;
}

function renderBoard(snapshot: GameSnapshot): string[] {
  const top = `+${"--".repeat(snapshot.board[0]!.length)}+`;
  return [
    top,
    ...snapshot.board.map((row) =>
      `|${row.map((cell) => cell === null ? " ." : ` ${cell}`).join("")}|`),
    top,
  ];
}

export function renderTerminal(
  snapshot: GameSnapshot,
  selection: DropIntervalSelection,
): string {
  const state = snapshot.gameOver
    ? "GAME OVER - press R to restart or Q to quit"
    : snapshot.paused
      ? "PAUSED"
      : "PLAYING";
  return [
    clearScreen,
    "Terminal Tetris",
    "",
    ...renderBoard(snapshot),
    "",
    `Score: ${snapshot.score}  Lines: ${snapshot.lines}  Level: ${snapshot.level}`,
    `Gravity: ${statusLine(selection)}`,
    `Board pressure: ${snapshot.dropContext.boardPressure.toFixed(2)}`,
    `State: ${state}`,
    "",
    "Controls: arrows/A,D move | up/W rotate | down/S soft drop",
    "          space hard drop | P pause | R restart | Q quit",
  ].join("\n");
}

export async function runTerminalTetris(
  options: TerminalTetrisOptions = {},
): Promise<void> {
  const input = options.input ?? process.stdin;
  const output = options.output ?? process.stdout;
  if (input.isTTY !== true || output.isTTY !== true || input.setRawMode === undefined) {
    throw new Error("Interactive Tetris requires a TTY terminal.");
  }

  const provider = options.provider ?? new LocalDropIntervalProvider();
  let game = new TetrisGame(options.game);
  let selection = await provider.select(
    game.snapshot().dropContext,
    options.signal,
  );
  options.signal?.throwIfAborted();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  let resolveStopped: (() => void) | undefined;
  let refreshGeneration = 0;
  let refreshController: AbortController | undefined;

  const render = (): void => {
    output.write(renderTerminal(game.snapshot(), selection));
  };

  const stop = (): void => {
    if (stopped) return;
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
    refreshController?.abort();
    input.removeListener("keypress", onKeypress);
    options.signal?.removeEventListener("abort", stop);
    input.setRawMode?.(false);
    input.pause();
    output.write(`${showCursor}\n`);
    resolveStopped?.();
  };

  const schedule = (): void => {
    if (timer !== undefined) clearTimeout(timer);
    if (stopped || game.paused || game.gameOver) return;
    timer = setTimeout(() => {
      timer = undefined;
      const update = game.tick();
      afterUpdate(update);
      schedule();
    }, selection.intervalMs);
  };

  const refreshInterval = (): void => {
    refreshGeneration += 1;
    const generation = refreshGeneration;
    refreshController?.abort();
    refreshController = new AbortController();
    const signal = options.signal === undefined
      ? refreshController.signal
      : AbortSignal.any([options.signal, refreshController.signal]);
    void provider.select(game.snapshot().dropContext, signal).then(
      (next) => {
        if (stopped || generation !== refreshGeneration) return;
        selection = next;
        render();
        schedule();
      },
      (error: unknown) => {
        if (stopped || generation !== refreshGeneration || signal.aborted) return;
        const message = error instanceof Error ? error.message : String(error);
        selection = {
          intervalMs: localDropInterval(game.level),
          source: "local-fallback",
          status: `provider error: ${message}`,
        };
        render();
        schedule();
      },
    );
  };

  const afterUpdate = (update: GameUpdate): void => {
    if (update.changed) render();
    if (update.locked && !update.gameOver) refreshInterval();
  };

  const restart = (): void => {
    game = new TetrisGame(options.game);
    refreshInterval();
    render();
    schedule();
  };

  const onKeypress = (_text: string, key: Key): void => {
    if (key.ctrl === true && key.name === "c") {
      stop();
      return;
    }
    if (key.name === "q") {
      stop();
      return;
    }
    if (key.name === "r") {
      restart();
      return;
    }
    if (key.name === "p") {
      game.setPaused(!game.paused);
      render();
      schedule();
      return;
    }
    if (game.paused || game.gameOver) return;

    let update: GameUpdate | undefined;
    switch (key.name) {
      case "left":
      case "a":
        update = game.moveLeft();
        break;
      case "right":
      case "d":
        update = game.moveRight();
        break;
      case "up":
      case "w":
        update = game.rotateClockwise();
        break;
      case "down":
      case "s":
        update = game.softDrop();
        break;
      case "space":
        update = game.hardDrop();
        break;
    }
    if (update !== undefined) {
      afterUpdate(update);
      schedule();
    }
  };

  emitKeypressEvents(input);
  input.setRawMode(true);
  input.resume();
  input.on("keypress", onKeypress);
  options.signal?.addEventListener("abort", stop, { once: true });
  output.write(hideCursor);
  render();
  schedule();

  await new Promise<void>((resolve) => {
    resolveStopped = resolve;
    if (stopped) resolve();
  });
}
