export interface DropIntervalContext {
  readonly boardPressure: number;
  readonly currentLevel: number;
  readonly recentPlacementTimeMs: number;
  readonly recoveryFailures: number;
  readonly sessionId: string;
}

export type DropIntervalSource =
  | "local"
  | "flaggo"
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
