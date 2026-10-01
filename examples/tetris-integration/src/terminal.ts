import { emitKeypressEvents, type Key } from "node:readline";

import {
  dropIntervalRefreshIntervalMs,
  type DropIntervalProvider,
  type DropIntervalSelection,
} from "./drop-interval.js";
import type { GameSnapshot } from "./game.js";
import {
  TetrisSession,
  type TetrisSessionCommand,
  type TetrisSessionGameOptions,
  type TetrisSessionInstrumentation,
  type TetrisSessionState,
  type TetrisSessionTransition,
} from "./session.js";

export interface TerminalTetrisOptions {
  readonly provider?: DropIntervalProvider;
  readonly signal?: AbortSignal;
  readonly input?: NodeJS.ReadStream;
  readonly output?: NodeJS.WriteStream;
  readonly game?: TetrisSessionGameOptions;
  readonly instrumentation?: TetrisSessionInstrumentation;
  readonly now?: () => number;
  readonly policyRefreshIntervalMs?: number;
}

const clearScreen = "\u001b[2J\u001b[H";
const hideCursor = "\u001b[?25l";
const showCursor = "\u001b[?25h";

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

  const policyRefreshIntervalMs = positiveDuration(
    options.policyRefreshIntervalMs,
    dropIntervalRefreshIntervalMs,
    "policy refresh interval",
  );
  const session = new TetrisSession({
    ...(options.provider === undefined ? {} : { provider: options.provider }),
    ...(options.game === undefined ? {} : { game: options.game }),
    ...(options.instrumentation === undefined
      ? {}
      : { instrumentation: options.instrumentation }),
    ...(options.now === undefined ? {} : { now: options.now }),
  });
  let gravityTimer: ReturnType<typeof setTimeout> | undefined;
  let policyTimer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  let resolveStopped: (() => void) | undefined;

  const render = (state: TetrisSessionState = session.snapshot()): void => {
    output.write(renderTerminal(state, state.dropInterval));
  };

  const stopPolicyRefresh = (): void => {
    if (policyTimer !== undefined) clearTimeout(policyTimer);
    policyTimer = undefined;
    session.cancelPolicyRefresh();
  };

  const stop = (): void => {
    if (stopped) return;
    stopped = true;
    if (gravityTimer !== undefined) clearTimeout(gravityTimer);
    stopPolicyRefresh();
    session.close();
    input.removeListener("keypress", onKeypress);
    options.signal?.removeEventListener("abort", stop);
    input.setRawMode?.(false);
    input.pause();
    output.write(`${showCursor}\n`);
    resolveStopped?.();
  };

  const scheduleGravity = (): void => {
    if (gravityTimer !== undefined) clearTimeout(gravityTimer);
    const state = session.snapshot();
    if (stopped || state.status !== "playing") return;
    gravityTimer = setTimeout(() => {
      gravityTimer = undefined;
      afterTransition(session.dispatch("gravity-tick"));
      scheduleGravity();
    }, state.dropInterval.intervalMs);
  };

  const refreshPolicy = (): void => {
    const state = session.snapshot();
    if (stopped || state.status !== "playing") return;
    void session.refreshPolicy(options.signal).then((transition) => {
      if (!stopped && transition.changed) render(transition.state);
    });
  };

  const schedulePolicyRefresh = (): void => {
    const state = session.snapshot();
    if (stopped || state.status !== "playing") return;
    policyTimer = setTimeout(() => {
      policyTimer = undefined;
      refreshPolicy();
      schedulePolicyRefresh();
    }, policyRefreshIntervalMs);
  };

  const startPolicyRefresh = (): void => {
    stopPolicyRefresh();
    if (stopped || session.snapshot().status !== "playing") return;
    refreshPolicy();
    schedulePolicyRefresh();
  };

  const afterTransition = (transition: TetrisSessionTransition): void => {
    if (transition.state.gameOver) stopPolicyRefresh();
    if (transition.changed) render(transition.state);
  };

  const restart = (): void => {
    stopPolicyRefresh();
    const transition = session.dispatch("restart");
    render(transition.state);
    scheduleGravity();
    startPolicyRefresh();
  };

  const togglePaused = (): void => {
    const command = session.snapshot().paused ? "resume" : "pause";
    const transition = session.dispatch(command);
    if (transition.state.paused) {
      stopPolicyRefresh();
    } else {
      startPolicyRefresh();
    }
    render(transition.state);
    scheduleGravity();
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
      togglePaused();
      return;
    }

    const state = session.snapshot();
    if (state.status !== "playing") return;
    const command = gameCommand(key);
    if (command !== undefined) {
      afterTransition(session.dispatch(command));
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

function gameCommand(key: Key): TetrisSessionCommand | undefined {
  switch (key.name) {
    case "left":
    case "a":
      return "move-left";
    case "right":
    case "d":
      return "move-right";
    case "up":
    case "w":
      return "rotate-clockwise";
    case "down":
    case "s":
      return "soft-drop";
    case "space":
      return "hard-drop";
    default:
      return undefined;
  }
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
