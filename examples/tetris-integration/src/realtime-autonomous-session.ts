import { dropIntervalRefreshIntervalMs } from "./drop-interval.js";
import { GreedyTetrisPlayer } from "./greedy-player.js";
import {
  TetrisSession,
  type TetrisSessionTransition,
} from "./session.js";
import type { AutonomousTetrisOptions, AutonomousTetrisResult } from "./autonomous-session.js";

export interface RealtimeAutonomousTetrisOptions extends AutonomousTetrisOptions {
  readonly now?: () => number;
}

export async function runRealtimeAutonomousTetrisSession(
  options: RealtimeAutonomousTetrisOptions,
): Promise<AutonomousTetrisResult> {
  if (!Number.isFinite(options.durationMs) || options.durationMs <= 0) {
    throw new RangeError("An autonomous episode requires a finite positive duration.");
  }
  options.signal?.throwIfAborted();
  const readClock = options.now ?? (() => performance.now());
  const startedAt = readClock();
  let previousTime = startedAt;
  const now = (): number => {
    const value = readClock();
    if (!Number.isFinite(value) || value < previousTime) {
      throw new RangeError("Real-time episode timestamps must be finite and chronological.");
    }
    previousTime = value;
    return Math.min(value - startedAt, options.durationMs);
  };
  const controller = new AbortController();
  const signal = options.signal === undefined ? controller.signal
    : AbortSignal.any([controller.signal, options.signal]);
  const session = new TetrisSession({
    ...(options.provider === undefined ? {} : { provider: options.provider }),
    ...(options.instrumentation === undefined ? {} : { instrumentation: options.instrumentation }),
    game: {
      ...(options.pieceSource === undefined ? {} : { pieceSource: options.pieceSource }),
      ...(options.sessionId === undefined ? {} : { sessionId: options.sessionId }),
    },
    fallbackIntervalMs: options.dropIntervalMs ?? 800,
    policyApplication: "piece-boundary",
    now,
  });
  let observedInterval = session.snapshot().dropInterval.intervalMs;
  let gravityAt = observedInterval;
  let policyAt = options.provider === undefined ? Infinity : 0;
  let pending: Promise<void> | undefined;
  let failure: { readonly error: unknown } | undefined;
  let stopping = false;
  let wake: (() => void) | undefined;
  let piecesLocked = 0;
  let hardDrops = 0;
  const observe = (transition?: TetrisSessionTransition): void => {
    const state = session.snapshot();
    if (state.dropInterval.intervalMs !== observedInterval) {
      observedInterval = state.dropInterval.intervalMs;
      gravityAt = now() + observedInterval;
    }
    options.onState?.(state, now(), transition);
  };
  const refresh = (): void => {
    if (pending !== undefined) return;
    const task = session.refreshPolicy(signal).then((transition) => {
      if (stopping) return;
      observe(transition);
    }).catch((error: unknown) => {
      failure = { error };
    }).finally(() => {
      if (pending === task) pending = undefined;
      wake?.();
    });
    pending = task;
  };
  try {
    const player = new GreedyTetrisPlayer(session, options);
    observe();
    while (now() < options.durationMs && session.snapshot().status === "playing") {
      signal.throwIfAborted();
      if (failure !== undefined) throw failure.error;
      const at = now();
      const actionAt = player.nextActionAt(at) ?? Infinity;
      const nextAt = Math.min(gravityAt, actionAt, policyAt, options.durationMs);
      if (nextAt > at) {
        await new Promise<void>((resolve) => {
          const finish = (): void => {
            clearTimeout(timer);
            signal.removeEventListener("abort", finish);
            wake = undefined;
            resolve();
          };
          const timer = setTimeout(finish, nextAt - at);
          wake = finish;
          signal.addEventListener("abort", finish, { once: true });
          if (signal.aborted) finish();
        });
        continue;
      }
      if (nextAt === options.durationMs) break;
      if (nextAt === gravityAt) {
        const transition = session.dispatch("gravity-tick");
        if (transition.update?.locked === true) piecesLocked += 1;
        gravityAt = now() + transition.state.dropInterval.intervalMs;
        observe(transition);
      } else if (nextAt === actionAt) {
        const command = player.nextCommand(at);
        if (command !== undefined) {
          const transition = session.dispatch(command);
          if (transition.update?.locked === true) {
            piecesLocked += 1;
            if (command === "hard-drop") hardDrops += 1;
            gravityAt = now() + transition.state.dropInterval.intervalMs;
          }
          observe(transition);
        }
      } else {
        observe();
        refresh();
        policyAt = now() + dropIntervalRefreshIntervalMs;
      }
    }
    signal.throwIfAborted();
    if (failure !== undefined) throw failure.error;
    observe();
    return { state: session.snapshot(), elapsedMs: now(), piecesLocked, hardDrops };
  } finally {
    stopping = true;
    controller.abort(new Error("Autonomous episode closed."));
    wake?.();
    session.close();
    await pending;
    observe();
  }
}
