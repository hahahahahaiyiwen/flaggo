import { randomUUID } from "node:crypto";

import {
  dropIntervalObservationWindowMs,
  LocalDropIntervalProvider,
  localDropInterval,
  RollingDropIntervalContext,
  type DropIntervalContext,
  type DropIntervalProvider,
  type DropIntervalSelection,
} from "./drop-interval.js";
import {
  TetrisGame,
  type GameSnapshot,
  type GameUpdate,
  type TetrisGameOptions,
} from "./game.js";

export type TetrisSessionCommand =
  | "move-left"
  | "move-right"
  | "rotate-clockwise"
  | "soft-drop"
  | "hard-drop"
  | "gravity-tick"
  | "pause"
  | "resume"
  | "restart";

export type TetrisSessionStatus =
  | "playing"
  | "paused"
  | "game-over"
  | "closed";

export interface TetrisSessionGameOptions
  extends Omit<TetrisGameOptions, "now"> {}

export interface TetrisSessionOptions {
  readonly provider?: DropIntervalProvider;
  readonly game?: TetrisSessionGameOptions;
  readonly now?: () => number;
}

export interface TetrisSessionState extends GameSnapshot {
  readonly revision: number;
  readonly sessionId: string;
  readonly status: TetrisSessionStatus;
  readonly dropInterval: DropIntervalSelection;
}

export interface TetrisSessionTransition {
  readonly operation: TetrisSessionCommand | "refresh-policy";
  readonly changed: boolean;
  readonly state: TetrisSessionState;
  readonly update?: GameUpdate;
  readonly policyContext?: DropIntervalContext;
  readonly reason?: "closed" | "inactive" | "in-flight" | "cancelled";
}

export interface TetrisSessionApi {
  snapshot(): TetrisSessionState;
  dispatch(command: TetrisSessionCommand): TetrisSessionTransition;
  refreshPolicy(signal?: AbortSignal): Promise<TetrisSessionTransition>;
  cancelPolicyRefresh(): boolean;
  close(): void;
}

interface PolicyRefresh {
  readonly controller: AbortController;
  readonly promise: Promise<TetrisSessionTransition>;
}

export class TetrisSession implements TetrisSessionApi {
  private readonly gameOptions: TetrisSessionGameOptions;
  private readonly now: () => number;
  private readonly provider: DropIntervalProvider;
  private readonly sessionId: string;
  private game: TetrisGame;
  private observationWindow: RollingDropIntervalContext;
  private lastObservedAt: number;
  private selection: DropIntervalSelection;
  private revisionValue = 0;
  private policyGeneration = 0;
  private policyRefresh: PolicyRefresh | undefined;
  private closed = false;

  constructor(options: TetrisSessionOptions = {}) {
    this.gameOptions = { ...(options.game ?? {}) };
    this.now = options.now ?? Date.now;
    this.provider = options.provider ?? new LocalDropIntervalProvider();
    this.sessionId = this.gameOptions.sessionId ?? `tetris-${randomUUID()}`;
    this.lastObservedAt = this.currentTime();
    this.game = this.createGame(this.lastObservedAt);
    this.observationWindow = this.createObservationWindow(
      this.lastObservedAt,
    );
    this.selection = localSelection(this.game.level);
  }

  snapshot(): TetrisSessionState {
    const game = this.game.snapshot();
    return {
      ...game,
      revision: this.revisionValue,
      sessionId: game.dropObservation.sessionId,
      status: this.status(),
      dropInterval: { ...this.selection },
    };
  }

  dispatch(command: TetrisSessionCommand): TetrisSessionTransition {
    this.assertOpen();
    const observedAt = this.currentTime();
    switch (command) {
      case "pause":
        return this.setPaused(true, command, observedAt);
      case "resume":
        return this.setPaused(false, command, observedAt);
      case "restart":
        return this.restart(command, observedAt);
      default:
        return this.applyGameCommand(command, observedAt);
    }
  }

  async refreshPolicy(
    externalSignal?: AbortSignal,
  ): Promise<TetrisSessionTransition> {
    this.assertOpen();
    if (this.game.paused || this.game.gameOver) {
      return this.transition("refresh-policy", false, {
        reason: "inactive",
      });
    }
    if (this.policyRefresh !== undefined) {
      return this.transition("refresh-policy", false, {
        reason: "in-flight",
      });
    }
    if (externalSignal?.aborted === true) {
      return this.transition("refresh-policy", false, {
        reason: "cancelled",
      });
    }

    const observedAt = this.currentTime();
    this.assertChronological(observedAt);
    const context = this.observationWindow.snapshot(
      this.game.snapshot().dropObservation,
      observedAt,
    );
    this.lastObservedAt = observedAt;
    const generation = this.policyGeneration;
    const controller = new AbortController();
    const signal = externalSignal === undefined
      ? controller.signal
      : AbortSignal.any([externalSignal, controller.signal]);
    let abortListener: (() => void) | undefined;
    const cancellation = new Promise<void>((resolve) => {
      const onAbort = (): void => resolve();
      abortListener = onAbort;
      signal.addEventListener("abort", onAbort, { once: true });
      if (signal.aborted) onAbort();
    }).then(() =>
      this.transition("refresh-policy", false, {
        policyContext: context,
        reason: "cancelled",
      })
    );
    const providerResult = Promise.resolve()
      .then(() => {
        if (signal.aborted) return undefined;
        return this.provider.select(context, signal);
      })
      .then(
        (refreshed) => ({ kind: "success" as const, refreshed }),
        (error: unknown) => ({ kind: "error" as const, error }),
      );
    const selection = providerResult.then((result) => {
      if (!this.canApplyPolicy(generation, signal)) {
        return this.transition("refresh-policy", false, {
          policyContext: context,
          reason: "cancelled",
        });
      }
      if (result.kind === "error") {
        const error = result.error;
        const message = error instanceof Error ? error.message : String(error);
        return this.applyPolicySelection({
          intervalMs: localDropInterval(this.game.level),
          source: "local-fallback",
          status: `provider error: ${message}`,
        }, context);
      }
      if (result.refreshed === undefined) {
        return this.transition("refresh-policy", false, {
          policyContext: context,
          reason: "cancelled",
        });
      }
      return this.applyPolicySelection(result.refreshed, context);
    });
    const tracked = Promise.race([selection, cancellation]).finally(() => {
      if (abortListener !== undefined) {
        signal.removeEventListener("abort", abortListener);
      }
      if (this.policyRefresh?.promise === tracked) {
        this.policyRefresh = undefined;
      }
    });
    this.policyRefresh = { controller, promise: tracked };
    return tracked;
  }

  cancelPolicyRefresh(): boolean {
    if (this.policyRefresh === undefined) return false;
    this.policyGeneration += 1;
    this.policyRefresh.controller.abort();
    this.policyRefresh = undefined;
    return true;
  }

  close(): void {
    if (this.closed) return;
    this.cancelPolicyRefresh();
    this.closed = true;
    this.revisionValue += 1;
  }

  private applyGameCommand(
    command: Exclude<
      TetrisSessionCommand,
      "pause" | "resume" | "restart"
    >,
    observedAt: number,
  ): TetrisSessionTransition {
    this.assertChronological(observedAt);
    const update = this.gameUpdate(command, observedAt);
    this.observationWindow.record(
      this.game.snapshot().dropObservation,
      observedAt,
      update.locked,
    );
    this.lastObservedAt = observedAt;

    let selectionChanged = false;
    if (
      update.locked
      && (
        this.selection.source === "local"
        || this.selection.source === "local-fallback"
      )
    ) {
      const next = {
        ...this.selection,
        intervalMs: localDropInterval(this.game.level),
      };
      selectionChanged = !sameSelection(this.selection, next);
      this.selection = next;
    }
    if (update.gameOver) this.cancelPolicyRefresh();

    const changed = update.changed
      || update.recoveryFailureRecorded
      || selectionChanged;
    if (changed) this.revisionValue += 1;
    return this.transition(command, changed, { update });
  }

  private gameUpdate(
    command: Exclude<
      TetrisSessionCommand,
      "pause" | "resume" | "restart"
    >,
    observedAt: number,
  ): GameUpdate {
    switch (command) {
      case "move-left":
        return this.game.moveLeft();
      case "move-right":
        return this.game.moveRight();
      case "rotate-clockwise":
        return this.game.rotateClockwise();
      case "soft-drop":
        return this.game.softDrop(observedAt);
      case "hard-drop":
        return this.game.hardDrop(observedAt);
      case "gravity-tick":
        return this.game.tick(observedAt);
    }
  }

  private setPaused(
    paused: boolean,
    command: "pause" | "resume",
    observedAt: number,
  ): TetrisSessionTransition {
    if (this.game.gameOver || this.game.paused === paused) {
      return this.transition(command, false);
    }
    this.assertChronological(observedAt);
    if (paused) {
      this.cancelPolicyRefresh();
    }
    this.game.setPaused(paused);
    this.lastObservedAt = observedAt;
    if (!paused) {
      this.observationWindow = this.createObservationWindow(observedAt);
    }
    this.revisionValue += 1;
    return this.transition(command, true);
  }

  private restart(
    command: "restart",
    observedAt: number,
  ): TetrisSessionTransition {
    this.assertChronological(observedAt);
    this.cancelPolicyRefresh();
    this.game = this.createGame(observedAt);
    this.lastObservedAt = observedAt;
    this.observationWindow = this.createObservationWindow(observedAt);
    this.selection = localSelection(this.game.level);
    this.revisionValue += 1;
    return this.transition(command, true);
  }

  private applyPolicySelection(
    refreshed: DropIntervalSelection,
    context: DropIntervalContext,
  ): TetrisSessionTransition {
    const next = updatedSelection(
      this.selection,
      refreshed,
      this.game.level,
    );
    const changed = !sameSelection(this.selection, next);
    this.selection = next;
    if (changed) this.revisionValue += 1;
    return this.transition("refresh-policy", changed, {
      policyContext: context,
    });
  }

  private transition(
    operation: TetrisSessionTransition["operation"],
    changed: boolean,
    details: Omit<
      TetrisSessionTransition,
      "operation" | "changed" | "state"
    > = {},
  ): TetrisSessionTransition {
    return {
      operation,
      changed,
      state: this.snapshot(),
      ...details,
    };
  }

  private createGame(startedAt: number): TetrisGame {
    return new TetrisGame({
      ...this.gameOptions,
      sessionId: this.sessionId,
      startedAt,
      now: this.now,
    });
  }

  private createObservationWindow(
    observedAt: number,
  ): RollingDropIntervalContext {
    return new RollingDropIntervalContext(
      this.game.snapshot().dropObservation,
      observedAt,
      dropIntervalObservationWindowMs,
    );
  }

  private currentTime(): number {
    const value = this.now();
    if (!Number.isFinite(value)) {
      throw new RangeError("Tetris session timestamps must be finite.");
    }
    return value;
  }

  private assertChronological(observedAt: number): void {
    if (observedAt < this.lastObservedAt) {
      throw new RangeError("Tetris session timestamps must be chronological.");
    }
  }

  private status(): TetrisSessionStatus {
    if (this.closed) return "closed";
    if (this.game.gameOver) return "game-over";
    return this.game.paused ? "paused" : "playing";
  }

  private canApplyPolicy(
    generation: number,
    signal: AbortSignal,
  ): boolean {
    return !this.closed
      && !signal.aborted
      && generation === this.policyGeneration
      && !this.game.paused
      && !this.game.gameOver;
  }

  private assertOpen(): void {
    if (this.closed) {
      throw new Error("The Tetris session is closed.");
    }
  }
}

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
  currentLevel: number,
): DropIntervalSelection {
  if (
    refreshed.source !== "local-fallback"
    || (
      current.source !== "flaggo"
      && current.source !== "flaggo-cached"
    )
  ) {
    return refreshed.source === "local"
        || refreshed.source === "local-fallback"
      ? {
        ...refreshed,
        intervalMs: localDropInterval(currentLevel),
      }
      : { ...refreshed };
  }
  return {
    intervalMs: current.intervalMs,
    source: "flaggo-cached",
    status: `retained after refresh failure: ${refreshed.status}`,
  };
}

function sameSelection(
  left: DropIntervalSelection,
  right: DropIntervalSelection,
): boolean {
  return left.intervalMs === right.intervalMs
    && left.source === right.source
    && left.status === right.status;
}
