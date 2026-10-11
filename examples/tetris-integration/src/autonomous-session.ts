import { dropIntervalRefreshIntervalMs, type DropIntervalProvider } from "./drop-interval.js";
import { type PieceSource } from "./game.js";
import { GreedyTetrisPlayer, type GreedyPlayerPacing } from "./greedy-player.js";
import {
  TetrisSession,
  type TetrisSessionInstrumentation,
  type TetrisSessionState,
  type TetrisSessionTransition,
} from "./session.js";

export interface AutonomousTetrisOptions extends GreedyPlayerPacing {
  readonly durationMs: number;
  readonly dropIntervalMs?: number;
  readonly provider?: DropIntervalProvider;
  readonly pieceSource?: PieceSource;
  readonly sessionId?: string;
  readonly instrumentation?: TetrisSessionInstrumentation;
  readonly signal?: AbortSignal;
  readonly onState?: (
    state: TetrisSessionState,
    observedAtMs: number,
    transition?: TetrisSessionTransition,
  ) => void;
}

export interface AutonomousTetrisResult {
  readonly state: TetrisSessionState;
  readonly elapsedMs: number;
  readonly piecesLocked: number;
  readonly hardDrops: number;
}

export async function runAutonomousTetrisSession(
  options: AutonomousTetrisOptions,
): Promise<AutonomousTetrisResult> {
  if (!Number.isFinite(options.durationMs) || options.durationMs <= 0) {
    throw new RangeError("An autonomous episode requires a finite positive duration.");
  }
  options.signal?.throwIfAborted();
  let now = 0;
  const session = new TetrisSession({
    ...(options.provider === undefined ? {} : { provider: options.provider }),
    ...(options.instrumentation === undefined ? {} : { instrumentation: options.instrumentation }),
    game: {
      ...(options.pieceSource === undefined ? {} : { pieceSource: options.pieceSource }),
      ...(options.sessionId === undefined ? {} : { sessionId: options.sessionId }),
    },
    fallbackIntervalMs: options.dropIntervalMs ?? 800,
    now: () => now,
  });
  let piecesLocked = 0;
  let hardDrops = 0;
  try {
    const player = new GreedyTetrisPlayer(session, options);
    options.onState?.(session.snapshot(), now);
    if (options.provider !== undefined) {
      const transition = await session.refreshPolicy(options.signal);
      options.signal?.throwIfAborted();
      options.onState?.(transition.state, now, transition);
    }
    let gravityAt = now + session.snapshot().dropInterval.intervalMs;
    let policyAt = dropIntervalRefreshIntervalMs;
    while (now < options.durationMs && session.snapshot().status === "playing") {
      options.signal?.throwIfAborted();
      const actionAt = player.nextActionAt(now) ?? Infinity;
      now = Math.min(gravityAt, actionAt, options.durationMs);
      let locked = false;
      if (now === gravityAt) {
        const transition = session.dispatch("gravity-tick");
        options.onState?.(transition.state, now, transition);
        locked = transition.update?.locked === true;
        if (locked) piecesLocked += 1;
        gravityAt = now + transition.state.dropInterval.intervalMs;
      }
      if (session.snapshot().status === "playing") {
        const command = player.nextCommand(now);
        if (command !== undefined) {
          const transition = session.dispatch(command);
          options.onState?.(transition.state, now, transition);
          if (transition.update?.locked === true) {
            locked = true;
            piecesLocked += 1;
            if (command === "hard-drop") hardDrops += 1;
          }
        }
      }
      if (locked) {
        if (options.provider !== undefined && now >= policyAt
          && session.snapshot().status === "playing") {
          const transition = await session.refreshPolicy(options.signal);
          options.signal?.throwIfAborted();
          options.onState?.(transition.state, now, transition);
          policyAt = now + dropIntervalRefreshIntervalMs;
        }
        gravityAt = now + session.snapshot().dropInterval.intervalMs;
      }
    }
    options.onState?.(session.snapshot(), now);
    return { state: session.snapshot(), elapsedMs: now, piecesLocked, hardDrops };
  } finally {
    session.close();
    options.onState?.(session.snapshot(), now);
  }
}
