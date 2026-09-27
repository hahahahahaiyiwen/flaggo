import { emitKeypressEvents, type Key } from "node:readline";

import {
  dropIntervalObservationWindowMs,
  dropIntervalRefreshIntervalMs,
  LocalDropIntervalProvider,
  localDropInterval,
  RollingDropIntervalContext,
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
  readonly now?: () => number;
  readonly policyRefreshIntervalMs?: number;
}

const clearScreen = "\u001b[2J\u001b[H";
const hideCursor = "\u001b[?25l";
const showCursor = "\u001b[?25h";

function localSelection(currentLevel: number): DropIntervalSelection {
  return {
    intervalMs: localDropInterval(currentLevel),
    source: "local",
    status: "local gravity policy",
  };
}

function updatedSelection(
  current: DropIntervalSelection,
  refreshed: DropIntervalSelection,
): DropIntervalSelection {
  if (
    refreshed.source !== "local-fallback"
    || (
      current.source !== "flaggo"
      && current.source !== "flaggo-cached"
    )
  ) {
    return refreshed;
  }
  return {
    intervalMs: current.intervalMs,
    source: "flaggo-cached",
    status: `retained after refresh failure: ${refreshed.status}`,
  };
}

function statusLine(selection: DropIntervalSelection): string {
  const source = selection.source === "flaggo"
    ? "Flaggo"
    : selection.source === "flaggo-cached"
      ? "Flaggo cached"
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
    `Current board pressure: ${
      snapshot.dropObservation.boardPressure.toFixed(2)
    }`,
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
  options.signal?.throwIfAborted();

  const provider = options.provider ?? new LocalDropIntervalProvider();
  const now = options.now ?? Date.now;
  const policyRefreshIntervalMs = positiveDuration(
    options.policyRefreshIntervalMs,
    dropIntervalRefreshIntervalMs,
    "policy refresh interval",
  );
  let game = new TetrisGame(options.game);
  let observationWindow = new RollingDropIntervalContext(
    game.snapshot().dropObservation,
    now(),
    dropIntervalObservationWindowMs,
  );
  let selection = localSelection(game.level);
  let gravityTimer: ReturnType<typeof setTimeout> | undefined;
  let policyTimer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  let resolveStopped: (() => void) | undefined;
  let policyGeneration = 0;
  let policyRequestInFlight = false;
  let refreshController: AbortController | undefined;

  const render = (): void => {
    output.write(renderTerminal(game.snapshot(), selection));
  };

  const stopPolicyRefresh = (): void => {
    policyGeneration += 1;
    if (policyTimer !== undefined) clearTimeout(policyTimer);
    policyTimer = undefined;
    refreshController?.abort();
    refreshController = undefined;
    policyRequestInFlight = false;
  };

  const stop = (): void => {
    if (stopped) return;
    stopped = true;
    if (gravityTimer !== undefined) clearTimeout(gravityTimer);
    stopPolicyRefresh();
    input.removeListener("keypress", onKeypress);
    options.signal?.removeEventListener("abort", stop);
    input.setRawMode?.(false);
    input.pause();
    output.write(`${showCursor}\n`);
    resolveStopped?.();
  };

  const scheduleGravity = (): void => {
    if (gravityTimer !== undefined) clearTimeout(gravityTimer);
    if (stopped || game.paused || game.gameOver) return;
    gravityTimer = setTimeout(() => {
      gravityTimer = undefined;
      const update = game.tick();
      afterUpdate(update);
      scheduleGravity();
    }, selection.intervalMs);
  };

  const applyRefreshedSelection = (
    refreshed: DropIntervalSelection,
  ): void => {
    selection = updatedSelection(selection, refreshed);
    render();
  };

  const refreshPolicy = (): void => {
    if (
      stopped
      || game.paused
      || game.gameOver
      || policyRequestInFlight
    ) {
      return;
    }
    policyRequestInFlight = true;
    const generation = policyGeneration;
    const controller = new AbortController();
    refreshController = controller;
    const signal = options.signal === undefined
      ? controller.signal
      : AbortSignal.any([options.signal, controller.signal]);
    const context = observationWindow.snapshot(
      game.snapshot().dropObservation,
      now(),
    );
    void provider.select(context, signal).then(
      (refreshed) => {
        if (
          stopped
          || signal.aborted
          || generation !== policyGeneration
        ) {
          return;
        }
        applyRefreshedSelection(refreshed);
      },
      (error: unknown) => {
        if (
          stopped
          || signal.aborted
          || generation !== policyGeneration
        ) {
          return;
        }
        const message = error instanceof Error ? error.message : String(error);
        applyRefreshedSelection({
          intervalMs: localDropInterval(game.level),
          source: "local-fallback",
          status: `provider error: ${message}`,
        });
      },
    ).finally(() => {
      if (generation !== policyGeneration) return;
      policyRequestInFlight = false;
      refreshController = undefined;
    });
  };

  const schedulePolicyRefresh = (): void => {
    if (stopped || game.paused || game.gameOver) return;
    policyTimer = setTimeout(() => {
      policyTimer = undefined;
      refreshPolicy();
      schedulePolicyRefresh();
    }, policyRefreshIntervalMs);
  };

  const startPolicyRefresh = (): void => {
    stopPolicyRefresh();
    if (stopped || game.paused || game.gameOver) return;
    refreshPolicy();
    schedulePolicyRefresh();
  };

  const resetObservationWindow = (): void => {
    observationWindow = new RollingDropIntervalContext(
      game.snapshot().dropObservation,
      now(),
      dropIntervalObservationWindowMs,
    );
  };

  const afterUpdate = (update: GameUpdate): void => {
    const snapshot = game.snapshot();
    observationWindow.record(
      snapshot.dropObservation,
      now(),
      update.locked,
    );
    if (
      update.locked
      && (
        selection.source === "local"
        || selection.source === "local-fallback"
      )
    ) {
      selection = {
        ...selection,
        intervalMs: localDropInterval(snapshot.level),
      };
    }
    if (update.gameOver) stopPolicyRefresh();
    if (update.changed) render();
  };

  const restart = (): void => {
    stopPolicyRefresh();
    game = new TetrisGame(options.game);
    selection = localSelection(game.level);
    resetObservationWindow();
    render();
    scheduleGravity();
    startPolicyRefresh();
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
      if (game.paused) {
        stopPolicyRefresh();
      } else {
        resetObservationWindow();
        startPolicyRefresh();
      }
      render();
      scheduleGravity();
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
      scheduleGravity();
    }
  };

  emitKeypressEvents(input);
  input.setRawMode(true);
  input.resume();
  input.on("keypress", onKeypress);
  options.signal?.addEventListener("abort", stop, { once: true });
  output.write(hideCursor);
  render();
  scheduleGravity();
  startPolicyRefresh();

  await new Promise<void>((resolve) => {
    resolveStopped = resolve;
    if (stopped) resolve();
  });
}

function positiveDuration(
  value: number | undefined,
  defaultValue: number,
  description: string,
): number {
  const duration = value ?? defaultValue;
  if (!Number.isFinite(duration) || duration <= 0) {
    throw new RangeError(`The ${description} must be positive.`);
  }
  return duration;
}
