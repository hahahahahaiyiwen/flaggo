import { randomUUID } from "node:crypto";
import type { Meter } from "@opentelemetry/api";

import { validDropInterval, type DropIntervalContext } from "./drop-interval.js";
import type {
  TetrisSessionState,
  TetrisSessionTransition,
} from "./session.js";

export const experiencePressureBand = { minimum: 0.15, maximum: 0.45 } as const;
const windowDurationMs = 5_000;

export type AdaptiveAttributes = {
  readonly board_pressure_mean_5s: number;
  readonly board_pressure_max_5s: number;
  readonly hard_drop_ratio_5s: number;
  readonly placement_time_mean_ms_5s: number;
  readonly pieces_locked_5s: number;
  readonly active_play_ms_5s: number;
  readonly challenge_balance_5s: number;
  readonly lines_cleared_5s: number;
  readonly game_over_count_5s: number;
  readonly current_drop_interval_ms: number;
  readonly session_id: string;
  readonly exposure_id: string;
};

export interface ExperienceMetadata {
  readonly dataset: "live" | "training" | "evaluation";
  readonly origin: "human" | "greedy";
  readonly clock: "wall" | "virtual";
}

export interface ExperienceWindow extends ExperienceMetadata {
  readonly windowId: string;
  readonly input: AdaptiveAttributes;
  readonly intervalMs: number;
  readonly startAtMs: number;
  readonly endAtMs: number;
  readonly activePlayMs: number;
  readonly challengeBalance: number;
  readonly linesCleared: number;
  readonly gameOvers: number;
  readonly episodeComplete: boolean;
}

export interface ExperienceTotals {
  readonly activePlayMs: number;
  readonly balancedPlayMs: number;
  readonly linesCleared: number;
  readonly gameOvers: number;
}

interface Registration {
  readonly input: AdaptiveAttributes;
  readonly intervalMs: number;
}

interface ActiveWindow {
  readonly registration: Registration;
  readonly startAtMs: number;
  endAtMs: number;
  activePlayMs: number;
  balancedPlayMs: number;
  linesCleared: number;
  gameOvers: number;
}

interface ActiveSegment {
  readonly start: number;
  readonly end: number;
  readonly balanced: boolean;
}

interface LockEvent {
  readonly at: number;
  readonly hardDrop: boolean;
  readonly lines: number;
}

export class TetrisExperience {
  private latest: { state: TetrisSessionState; at: number } | undefined;
  private readonly registrations = new Map<string, Registration>();
  private applied: Registration | undefined;
  private window: ActiveWindow | undefined;
  private segments: ActiveSegment[] = [];
  private locks: LockEvent[] = [];
  private gameOvers: number[] = [];
  private windowSequence = 0;
  private completed = false;
  private readonly totalsValue = {
    activePlayMs: 0,
    balancedPlayMs: 0,
    linesCleared: 0,
    gameOvers: 0,
  };

  constructor(
    private readonly metadata: ExperienceMetadata,
    private readonly onWindow?: (window: ExperienceWindow) => void,
  ) {}

  observe(
    state: TetrisSessionState,
    at: number,
    transition?: TetrisSessionTransition,
  ): void {
    if (!Number.isFinite(at) || (this.latest !== undefined && at < this.latest.at)) {
      throw new RangeError("Experience timestamps must be finite and chronological.");
    }
    if (this.latest !== undefined && state.sessionId !== this.latest.state.sessionId) {
      throw new Error("An experience tracker belongs to one Tetris session.");
    }
    const previous = this.latest;
    // Failed reporting must not replay the elapsed time when the session closes.
    this.latest = { state, at };
    if (previous?.state.status === "playing" && at > previous.at) {
      const pressure = previous.state.dropObservation.boardPressure;
      const balanced = pressure >= experiencePressureBand.minimum
        && pressure <= experiencePressureBand.maximum;
      const duration = at - previous.at;
      this.segments.push({ start: previous.at, end: at, balanced });
      this.totalsValue.activePlayMs += duration;
      if (balanced) this.totalsValue.balancedPlayMs += duration;
      this.accumulate(previous.at, duration, balanced);
    }

    // A locking command belongs to the old exposure, even if it applies a queued policy.
    if (transition?.update?.locked === true) {
      const lines = transition.update.linesCleared;
      this.locks.push({ at, hardDrop: transition.operation === "hard-drop", lines });
      this.totalsValue.linesCleared += lines;
      this.ensureWindow(at);
      if (this.window !== undefined) this.window.linesCleared += lines;
    }
    if (state.gameOver && previous?.state.gameOver !== true) {
      this.gameOvers.push(at);
      this.totalsValue.gameOvers += 1;
      this.ensureWindow(at);
      if (this.window !== undefined) this.window.gameOvers += 1;
    }

    const exposureId = state.dropInterval.exposureId;
    if (exposureId !== this.applied?.input.exposure_id) {
      this.finishWindow();
      this.applied = exposureId === undefined ? undefined : this.registrations.get(exposureId);
      if (exposureId !== undefined && this.applied === undefined) {
        throw new Error(`Applied exposure '${exposureId}' has no registered decision.`);
      }
    }
    if (this.applied !== undefined && this.applied.intervalMs !== state.dropInterval.intervalMs) {
      throw new Error("The applied interval does not match its registered decision.");
    }
    if (state.status !== "playing") this.finishWindow();
    else if (this.window?.activePlayMs === windowDurationMs) this.finishWindow();

    if (
      transition?.operation === "restart"
      || transition?.operation === "resume"
    ) {
      this.segments = [];
      this.locks = [];
      this.gameOvers = [];
      this.completed = false;
    }
    const cutoff = at - windowDurationMs;
    this.segments = this.segments.filter((entry) => entry.end > cutoff);
    this.locks = this.locks.filter((entry) => entry.at >= cutoff);
    this.gameOvers = this.gameOvers.filter((entry) => entry >= cutoff);
    for (const id of this.registrations.keys()) {
      if (id !== exposureId && id !== state.pendingDropInterval?.exposureId) {
        this.registrations.delete(id);
      }
    }
  }

  runtimeInput(context: DropIntervalContext): AdaptiveAttributes {
    const latest = this.latest;
    if (latest === undefined || latest.state.sessionId !== context.sessionId) {
      throw new Error("Observe the session before building its adaptive decision input.");
    }
    const cutoff = latest.at - windowDurationMs;
    let activePlayMs = 0;
    let balancedPlayMs = 0;
    for (const segment of this.segments) {
      const duration = segment.end - Math.max(cutoff, segment.start);
      activePlayMs += duration;
      if (segment.balanced) balancedPlayMs += duration;
    }
    const hardDrops = this.locks.filter((entry) => entry.hardDrop).length;
    return {
      board_pressure_mean_5s: context.boardPressureMean5s,
      board_pressure_max_5s: context.boardPressureMax5s,
      hard_drop_ratio_5s: this.locks.length === 0 ? 0 : hardDrops / this.locks.length,
      placement_time_mean_ms_5s: context.placementTimeMeanMs5s,
      pieces_locked_5s: context.piecesLocked5s,
      active_play_ms_5s: activePlayMs,
      challenge_balance_5s: activePlayMs === 0 ? 0 : balancedPlayMs / activePlayMs,
      lines_cleared_5s: this.locks.reduce((sum, entry) => sum + entry.lines, 0),
      game_over_count_5s: this.gameOvers.length,
      current_drop_interval_ms: latest.state.dropInterval.intervalMs,
      session_id: context.sessionId,
      exposure_id: `exposure-${randomUUID()}`,
    };
  }

  registerDecision(input: AdaptiveAttributes, intervalMs: number): void {
    if (input.session_id !== this.latest?.state.sessionId
      || input.exposure_id.length === 0 || !validDropInterval(intervalMs)) {
      throw new Error("Register a bounded decision for this session and a nonempty exposure.");
    }
    if (this.registrations.has(input.exposure_id)) {
      throw new Error("A decision exposure must not be reused.");
    }
    this.registrations.set(input.exposure_id, {
      input: Object.freeze({ ...input }),
      intervalMs,
    });
  }

  completeEpisode(): void {
    if (this.latest?.state.status !== "closed" || this.applied === undefined) {
      throw new Error("Complete an episode only after closing a session with applied decisions.");
    }
    if (this.completed) return;
    this.ensureWindow(this.latest.at);
    this.finishWindow(true);
    this.completed = true;
  }

  totals(): ExperienceTotals {
    return { ...this.totalsValue };
  }

  private ensureWindow(at: number): void {
    if (this.window !== undefined || this.applied === undefined) return;
    this.window = {
      registration: this.applied,
      startAtMs: at,
      endAtMs: at,
      activePlayMs: 0,
      balancedPlayMs: 0,
      linesCleared: 0,
      gameOvers: 0,
    };
  }

  private accumulate(startAt: number, duration: number, balanced: boolean): void {
    let remaining = duration;
    let cursor = startAt;
    while (remaining > 0 && this.applied !== undefined) {
      this.ensureWindow(cursor);
      const window = this.window!;
      if (window.activePlayMs === windowDurationMs) {
        this.finishWindow();
        continue;
      }
      const slice = Math.min(remaining, windowDurationMs - window.activePlayMs);
      window.activePlayMs += slice;
      cursor += slice;
      window.endAtMs = cursor;
      if (balanced) window.balancedPlayMs += slice;
      remaining -= slice;
    }
  }

  private finishWindow(episodeComplete = false): void {
    const window = this.window;
    this.window = undefined;
    if (
      window === undefined
      || (window.activePlayMs === 0 && window.linesCleared === 0
        && window.gameOvers === 0 && !episodeComplete)
    ) return;
    this.onWindow?.({
      windowId: `${window.registration.input.exposure_id}-${++this.windowSequence}`,
      input: window.registration.input,
      intervalMs: window.registration.intervalMs,
      ...this.metadata,
      startAtMs: window.startAtMs,
      endAtMs: window.endAtMs,
      activePlayMs: window.activePlayMs,
      challengeBalance: window.activePlayMs === 0 ? 0
        : window.balancedPlayMs / window.activePlayMs,
      linesCleared: window.linesCleared,
      gameOvers: window.gameOvers,
      episodeComplete,
    });
  }
}

export function createExperienceRecorder(meter: Meter): (window: ExperienceWindow) => void {
  const balance = meter.createGauge("tetris.experience.challenge_balance", { unit: "1" });
  const lines = meter.createGauge("tetris.experience.lines_cleared", { unit: "{line}" });
  const gameOvers = meter.createGauge("tetris.experience.game_over_count", { unit: "{game}" });
  const activePlay = meter.createGauge("tetris.experience.active_play_ms", { unit: "ms" });
  return (window) => {
    const attributes = {
      "tetris.session.id": window.input.session_id,
      "tetris.exposure.id": window.input.exposure_id,
      "tetris.experience.window.id": window.windowId,
      "tetris.experience.dataset": window.dataset,
      "tetris.experience.clock": window.clock,
      "tetris.player.origin": window.origin,
      "tetris.experience.window.start_ms": window.startAtMs,
      "tetris.experience.window.end_ms": window.endAtMs,
      "tetris.experience.episode_complete": window.episodeComplete,
      "tetris.drop_interval.ms": window.intervalMs,
      "tetris.input.board_pressure_mean_5s": window.input.board_pressure_mean_5s,
      "tetris.input.hard_drop_ratio_5s": window.input.hard_drop_ratio_5s,
      "tetris.input.pieces_locked_5s": window.input.pieces_locked_5s,
      "tetris.input.active_play_ms_5s": window.input.active_play_ms_5s,
    };
    balance.record(window.challengeBalance, attributes);
    lines.record(window.linesCleared, attributes);
    gameOvers.record(window.gameOvers, attributes);
    activePlay.record(window.activePlayMs, attributes);
  };
}
