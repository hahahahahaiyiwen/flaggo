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
    readonly board_pressure: number;
    readonly current_level: number;
    readonly recent_placement_time_ms: number;
    readonly recovery_failures: number;
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
            board_pressure: Math.max(0, Math.min(1, context.boardPressure)),
            current_level: Math.max(0, Math.min(20, context.currentLevel)),
            recent_placement_time_ms: Math.max(
              0,
              Math.min(2_000, context.recentPlacementTimeMs),
            ),
            recovery_failures: Math.max(
              0,
              Math.min(5, context.recoveryFailures),
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
