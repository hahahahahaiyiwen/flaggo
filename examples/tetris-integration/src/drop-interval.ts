export const dropIntervalObservationWindowMs = 5_000;
export const dropIntervalRefreshIntervalMs = 5_000;

export interface DropIntervalObservation {
  readonly boardPressure: number;
  readonly currentLevel: number;
  readonly placementTimeMs: number;
  readonly recoveryFailures: number;
  readonly sessionId: string;
}

export interface DropIntervalContext {
  readonly boardPressureMean5s: number;
  readonly boardPressureMax5s: number;
  readonly currentLevel: number;
  readonly placementTimeMeanMs5s: number;
  readonly recoveryFailures5s: number;
  readonly piecesLocked5s: number;
  readonly sessionId: string;
}

export type DropIntervalSource =
  | "local"
  | "flaggo"
  | "flaggo-cached"
  | "local-fallback";

export interface DropIntervalSelection {
  readonly intervalMs: number;
  readonly source: DropIntervalSource;
  readonly status: string;
}

export interface DropIntervalProvider {
  select(
    context: DropIntervalContext,
    signal?: AbortSignal,
  ): Promise<DropIntervalSelection>;
}

interface PressureChange {
  readonly observedAt: number;
  readonly value: number;
}

interface PlacementObservation {
  readonly observedAt: number;
  readonly placementTimeMs: number;
  readonly recoveryFailures: number;
}

export class RollingDropIntervalContext {
  private readonly startedAt: number;
  private readonly sessionId: string;
  private lastObservedAt: number;
  private pressureChanges: PressureChange[];
  private placementObservations: PlacementObservation[] = [];

  constructor(
    initial: DropIntervalObservation,
    observedAt: number,
    private readonly windowMs = dropIntervalObservationWindowMs,
  ) {
    assertTimestamp(observedAt);
    if (!Number.isFinite(windowMs) || windowMs <= 0) {
      throw new RangeError("The drop-interval observation window must be positive.");
    }
    this.startedAt = observedAt;
    this.lastObservedAt = observedAt;
    this.sessionId = initial.sessionId;
    this.pressureChanges = [{
      observedAt,
      value: boundedPressure(initial.boardPressure),
    }];
  }

  record(
    observation: DropIntervalObservation,
    observedAt: number,
    pieceLocked = false,
  ): void {
    this.assertObservation(observation, observedAt);
    const pressure = boundedPressure(observation.boardPressure);
    const last = this.pressureChanges[this.pressureChanges.length - 1]!;
    if (last.value !== pressure) {
      if (last.observedAt === observedAt) {
        this.pressureChanges[this.pressureChanges.length - 1] = {
          observedAt,
          value: pressure,
        };
      } else {
        this.pressureChanges.push({ observedAt, value: pressure });
      }
    }
    if (pieceLocked) {
      this.placementObservations.push({
        observedAt,
        placementTimeMs: Math.max(0, observation.placementTimeMs),
        recoveryFailures: Math.max(
          0,
          Math.trunc(observation.recoveryFailures),
        ),
      });
    }
    this.lastObservedAt = observedAt;
    this.prune(observedAt);
  }

  snapshot(
    observation: DropIntervalObservation,
    observedAt: number,
  ): DropIntervalContext {
    this.record(observation, observedAt);
    const windowStart = Math.max(
      this.startedAt,
      observedAt - this.windowMs,
    );
    const pressure = this.summarizePressure(windowStart, observedAt);
    const placements = this.placementObservations.filter(
      (entry) => entry.observedAt >= windowStart,
    );
    const placementTimeTotal = placements.reduce(
      (total, entry) => total + entry.placementTimeMs,
      0,
    );
    const recoveryFailures = placements.reduce(
      (total, entry) => total + entry.recoveryFailures,
      0,
    );

    return {
      boardPressureMean5s: pressure.mean,
      boardPressureMax5s: pressure.maximum,
      currentLevel: observation.currentLevel,
      placementTimeMeanMs5s: placements.length === 0
        ? 0
        : placementTimeTotal / placements.length,
      recoveryFailures5s: recoveryFailures,
      piecesLocked5s: placements.length,
      sessionId: this.sessionId,
    };
  }

  private assertObservation(
    observation: DropIntervalObservation,
    observedAt: number,
  ): void {
    assertTimestamp(observedAt);
    if (observation.sessionId !== this.sessionId) {
      throw new Error("A rolling drop-interval window cannot span game sessions.");
    }
    if (observedAt < this.lastObservedAt) {
      throw new RangeError("Drop-interval observations must be chronological.");
    }
  }

  private prune(observedAt: number): void {
    const windowStart = Math.max(
      this.startedAt,
      observedAt - this.windowMs,
    );
    while (
      this.pressureChanges.length > 1
      && this.pressureChanges[1]!.observedAt <= windowStart
    ) {
      this.pressureChanges.shift();
    }
    this.placementObservations = this.placementObservations.filter(
      (entry) => entry.observedAt >= windowStart,
    );
  }

  private summarizePressure(
    windowStart: number,
    observedAt: number,
  ): { readonly mean: number; readonly maximum: number } {
    let value = this.pressureChanges[0]!.value;
    let cursor = windowStart;
    let weightedTotal = 0;
    let maximum = value;

    for (const change of this.pressureChanges) {
      if (change.observedAt <= windowStart) {
        value = change.value;
        maximum = Math.max(maximum, value);
        continue;
      }
      if (change.observedAt > observedAt) break;
      weightedTotal += value * (change.observedAt - cursor);
      cursor = change.observedAt;
      value = change.value;
      maximum = Math.max(maximum, value);
    }
    weightedTotal += value * (observedAt - cursor);
    const duration = observedAt - windowStart;
    return {
      mean: duration === 0 ? value : weightedTotal / duration,
      maximum,
    };
  }
}

export function localDropInterval(currentLevel: number): number {
  const level = Number.isFinite(currentLevel)
    ? Math.min(20, Math.max(0, Math.trunc(currentLevel)))
    : 0;
  return Math.max(200, 800 - level * 50);
}

export class LocalDropIntervalProvider implements DropIntervalProvider {
  async select(
    context: DropIntervalContext,
  ): Promise<DropIntervalSelection> {
    return {
      intervalMs: localDropInterval(context.currentLevel),
      source: "local",
      status: "local gravity policy",
    };
  }
}

export function validDropInterval(value: unknown): value is number {
  return typeof value === "number"
    && Number.isFinite(value)
    && value >= 200
    && value <= 1_500
    && value % 50 === 0;
}

function assertTimestamp(value: number): void {
  if (!Number.isFinite(value)) {
    throw new RangeError("Drop-interval observation timestamps must be finite.");
  }
}

function boundedPressure(value: number): number {
  return Number.isFinite(value)
    ? Math.max(0, Math.min(1, value))
    : 0;
}
