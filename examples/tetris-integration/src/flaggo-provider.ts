import {
  SpanStatusCode,
  type Attributes,
  type Gauge,
  type Meter,
  type Span,
  type Tracer,
} from "@opentelemetry/api";
import type { Logger } from "@opentelemetry/api-logs";
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

type TetrisDropIntervalAttributes =
  TetrisDecisions["tetris.dropInterval"]["attributes"];

export interface FlaggoDropIntervalConfiguration {
  readonly baseUrl: string | URL;
  readonly contractDigest: Sha256Digest;
  readonly credential?: CredentialProvider;
  readonly fetch?: FetchLike;
  readonly retry?: RetryPolicy;
  readonly telemetry?: TetrisOpenTelemetry;
  readonly timeoutMs?: number;
}

export interface TetrisOpenTelemetry {
  readonly flaggoLogger: Logger;
  readonly logger: Logger;
  readonly meter: Meter;
  readonly tracer: Tracer;
}

interface TetrisPolicyMetrics {
  readonly boardPressureMean: Gauge;
  readonly boardPressureMax: Gauge;
  readonly currentLevel: Gauge;
  readonly placementTimeMean: Gauge;
  readonly recoveryFailures: Gauge;
  readonly piecesLocked: Gauge;
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
  const metrics = configuration.telemetry === undefined
    ? undefined
    : createPolicyMetrics(configuration.telemetry.meter);
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
    ...(configuration.telemetry === undefined
      ? {}
      : { telemetry: { logger: configuration.telemetry.flaggoLogger } }),
  });

  async function select(
    context: DropIntervalContext,
    attributes: TetrisDropIntervalAttributes,
    signal: AbortSignal | undefined,
    span: Span | undefined,
  ): Promise<DropIntervalSelection> {
    try {
      const response = await client.decide("tetris.dropInterval", {
        attributes,
      }, {
        ...(signal === undefined ? {} : { signal }),
      });
      if (!validDropInterval(response.value.result)) {
        throw new RangeError(
          "Flaggo returned a drop interval outside the contract bounds.",
        );
      }
      const evaluation = response.value.evaluation;
      const selection: DropIntervalSelection = {
        intervalMs: response.value.result,
        source: "flaggo",
        status: evaluation.source === "rule"
          ? `rule ${evaluation.rule}`
          : "contract default",
      };
      span?.setAttributes({
        "flaggo.contract.name": "tetris.dropInterval",
        "flaggo.contract.digest": response.value.contractDigest,
        "tetris.drop_interval.ms": selection.intervalMs,
        "tetris.drop_interval.source": selection.source,
      });
      configuration.telemetry?.logger.emit({
        eventName: "tetris.drop_interval.selected",
        attributes: {
          ...policyAttributes(attributes),
          "tetris.drop_interval.ms": selection.intervalMs,
          "tetris.drop_interval.source": selection.source,
          "flaggo.contract.name": "tetris.dropInterval",
          "flaggo.contract.digest": response.value.contractDigest,
          "flaggo.evaluation.source": evaluation.source,
          ...(evaluation.source === "rule"
            ? { "flaggo.evaluation.rule": evaluation.rule }
            : {}),
        },
      });
      return selection;
    } catch (error) {
      span?.recordException(error instanceof Error ? error : String(error));
      span?.setStatus({
        code: SpanStatusCode.ERROR,
        message: errorMessage(error),
      });
      if (signal?.aborted === true) throw error;
      const local = await fallback.select(context);
      const selection: DropIntervalSelection = {
        ...local,
        source: "local-fallback",
        status: `Flaggo unavailable: ${errorMessage(error)}`,
      };
      configuration.telemetry?.logger.emit({
        eventName: "tetris.drop_interval.selected",
        attributes: {
          ...policyAttributes(attributes),
          "tetris.drop_interval.ms": selection.intervalMs,
          "tetris.drop_interval.source": selection.source,
          "error.type": error instanceof Error ? error.name : "Error",
          "error.message": errorMessage(error),
        },
      });
      return selection;
    } finally {
      span?.end();
    }
  }

  return {
    async select(
      context: DropIntervalContext,
      signal?: AbortSignal,
    ): Promise<DropIntervalSelection> {
      const attributes = decisionAttributes(context);
      const telemetry = configuration.telemetry;
      return telemetry === undefined
        ? select(context, attributes, signal, undefined)
        : telemetry.tracer.startActiveSpan(
          "tetris.drop_interval.select",
          { attributes: policyAttributes(attributes) },
          (span) => {
            recordPolicyMetrics(metrics, attributes);
            return select(context, attributes, signal, span);
          },
        );
    },
  };
}

function createPolicyMetrics(meter: Meter): TetrisPolicyMetrics {
  return {
    boardPressureMean: meter.createGauge("tetris.board_pressure_mean_5s", {
      description: "Mean board pressure over the current five-second window.",
      unit: "1",
    }),
    boardPressureMax: meter.createGauge("tetris.board_pressure_max_5s", {
      description: "Maximum board pressure over the current five-second window.",
      unit: "1",
    }),
    currentLevel: meter.createGauge("tetris.current_level", {
      description: "Current Tetris level.",
      unit: "{level}",
    }),
    placementTimeMean: meter.createGauge(
      "tetris.placement_time_mean_ms_5s",
      {
        description: "Mean piece placement time over five seconds.",
        unit: "ms",
      },
    ),
    recoveryFailures: meter.createGauge("tetris.recovery_failures_5s", {
      description: "Recovery failures in the current five-second window.",
      unit: "{failure}",
    }),
    piecesLocked: meter.createGauge("tetris.pieces_locked_5s", {
      description: "Pieces locked in the current five-second window.",
      unit: "{piece}",
    }),
  };
}

function recordPolicyMetrics(
  metrics: TetrisPolicyMetrics | undefined,
  attributes: TetrisDropIntervalAttributes,
): void {
  if (metrics === undefined) return;
  const metricAttributes = { "tetris.session.id": attributes.session_id };
  metrics.boardPressureMean.record(
    attributes.board_pressure_mean_5s,
    metricAttributes,
  );
  metrics.boardPressureMax.record(
    attributes.board_pressure_max_5s,
    metricAttributes,
  );
  metrics.currentLevel.record(attributes.current_level, metricAttributes);
  metrics.placementTimeMean.record(
    attributes.placement_time_mean_ms_5s,
    metricAttributes,
  );
  metrics.recoveryFailures.record(
    attributes.recovery_failures_5s,
    metricAttributes,
  );
  metrics.piecesLocked.record(
    attributes.pieces_locked_5s,
    metricAttributes,
  );
}

function decisionAttributes(
  context: DropIntervalContext,
): TetrisDropIntervalAttributes {
  return {
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
  };
}

function policyAttributes(
  attributes: TetrisDropIntervalAttributes,
): Attributes {
  return {
    "tetris.board_pressure_mean_5s": attributes.board_pressure_mean_5s,
    "tetris.board_pressure_max_5s": attributes.board_pressure_max_5s,
    "tetris.current_level": attributes.current_level,
    "tetris.placement_time_mean_ms_5s":
      attributes.placement_time_mean_ms_5s,
    "tetris.recovery_failures_5s": attributes.recovery_failures_5s,
    "tetris.pieces_locked_5s": attributes.pieces_locked_5s,
    "tetris.session.id": attributes.session_id,
  };
}
