import {
  createDecisionClient,
  type CredentialProvider,
  type DecisionSpec,
  type FetchLike,
  type RetryPolicy,
  type Sha256Digest,
} from "@flaggo/sdk/runtime";

import {
  LocalDropIntervalProvider,
  validDropInterval,
  type DropIntervalContext,
  type DropIntervalProvider,
  type DropIntervalSelection,
} from "./drop-interval.js";

type TetrisDecisions = {
  readonly "tetris.dropInterval": DecisionSpec<{
    readonly board_pressure_mean_5s: number;
    readonly board_pressure_max_5s: number;
    readonly current_level: number;
    readonly placement_time_mean_ms_5s: number;
    readonly recovery_failures_5s: number;
    readonly pieces_locked_5s: number;
    readonly session_id: string;
  }, number>;
};

export interface FlaggoDropIntervalConfiguration {
  readonly baseUrl: string | URL;
  readonly contractDigest: Sha256Digest;
  readonly credential?: CredentialProvider;
  readonly fetch?: FetchLike;
  readonly retry?: RetryPolicy;
  readonly timeoutMs?: number;
}

function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const compact = message.replace(/\s+/gu, " ").trim();
  return compact.length <= 120 ? compact : `${compact.slice(0, 117)}...`;
}

export function createFlaggoDropIntervalProvider(
  configuration: FlaggoDropIntervalConfiguration,
): DropIntervalProvider {
  const fallback = new LocalDropIntervalProvider();
  const client = createDecisionClient<TetrisDecisions>({
    baseUrl: configuration.baseUrl,
    bindings: {
      "tetris.dropInterval": {
        contractDigest: configuration.contractDigest,
      },
    },
    ...(configuration.credential === undefined
      ? {}
      : { credential: configuration.credential }),
    ...(configuration.fetch === undefined
      ? {}
      : { fetch: configuration.fetch }),
    timeoutMs: configuration.timeoutMs ?? 1_500,
    retry: configuration.retry ?? {
      maxAttempts: 2,
      baseDelayMs: 50,
      maxDelayMs: 500,
    },
  });

  return {
    async select(
      context: DropIntervalContext,
      signal?: AbortSignal,
    ): Promise<DropIntervalSelection> {
      try {
        const response = await client.decide("tetris.dropInterval", {
          attributes: {
            board_pressure_mean_5s: Math.max(
              0,
              Math.min(1, context.boardPressureMean5s),
            ),
            board_pressure_max_5s: Math.max(
              0,
              Math.min(1, context.boardPressureMax5s),
            ),
            current_level: Math.max(0, Math.min(20, context.currentLevel)),
            placement_time_mean_ms_5s: Math.max(
              0,
              Math.min(60_000, context.placementTimeMeanMs5s),
            ),
            recovery_failures_5s: Math.max(
              0,
              Math.min(100, Math.trunc(context.recoveryFailures5s)),
            ),
            pieces_locked_5s: Math.max(
              0,
              Math.min(100, Math.trunc(context.piecesLocked5s)),
            ),
            session_id: context.sessionId,
          },
        }, {
          ...(signal === undefined ? {} : { signal }),
        });
        if (!validDropInterval(response.value.result)) {
          throw new RangeError(
            "Flaggo returned a drop interval outside the contract bounds.",
          );
        }
        const evaluation = response.value.evaluation;
        return {
          intervalMs: response.value.result,
          source: "flaggo",
          status: evaluation.source === "rule"
            ? `rule ${evaluation.rule}`
            : "contract default",
        };
      } catch (error) {
        if (signal?.aborted === true) throw error;
        const local = await fallback.select(context);
        return {
          ...local,
          source: "local-fallback",
          status: `Flaggo unavailable: ${errorMessage(error)}`,
        };
      }
    },
  };
}
