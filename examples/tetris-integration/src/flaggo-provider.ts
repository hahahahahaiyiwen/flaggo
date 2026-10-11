import { SpanStatusCode, type Span, type Tracer } from "@opentelemetry/api";
import type { Logger } from "@opentelemetry/api-logs";
import {
  createDecisionClient,
  type DecisionRuntimeConfiguration,
  type DecisionSpec,
  type FetchLike,
  type RetryPolicy,
} from "@flaggo/sdk/runtime";

import {
  validDropInterval,
  type DropIntervalProvider,
  type DropIntervalSelection,
} from "./drop-interval.js";
import type { TetrisSessionState } from "./session.js";
import type { AdaptiveAttributes, TetrisExperience } from "./experience.js";

type AdaptiveDecisions = {
  readonly "tetris.dropInterval": DecisionSpec<AdaptiveAttributes, number>;
};

export interface FlaggoDropIntervalConfiguration {
  readonly runtimeConfig: DecisionRuntimeConfiguration<AdaptiveDecisions>;
  readonly experience: TetrisExperience;
  readonly state: () => TetrisSessionState;
  readonly now: () => number;
  readonly fetch?: FetchLike;
  readonly random?: () => number;
  readonly retry?: RetryPolicy;
  readonly timeoutMs?: number;
  readonly telemetry?: TetrisPolicyOpenTelemetry;
}

export interface TetrisPolicyOpenTelemetry {
  readonly flaggoLogger: Logger;
  readonly logger: Logger;
  readonly tracer: Tracer;
}

export function createFlaggoDropIntervalProvider(
  options: FlaggoDropIntervalConfiguration,
): DropIntervalProvider {
  const client = createDecisionClient<AdaptiveDecisions>({
    runtimeConfig: options.runtimeConfig,
    ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
    ...(options.random === undefined ? {} : { random: options.random }),
    retry: options.retry ?? { maxAttempts: 1 },
    timeoutMs: options.timeoutMs ?? 1_500,
    ...(options.telemetry === undefined ? {}
      : { telemetry: { logger: options.telemetry.flaggoLogger } }),
  });

  const select = async (
    attributes: AdaptiveAttributes,
    signal: AbortSignal | undefined,
    span?: Span,
  ): Promise<DropIntervalSelection> => {
    try {
      const response = await client.decide("tetris.dropInterval", { attributes }, {
        ...(signal === undefined ? {} : { signal }),
      });
      const intervalMs = response.value.result;
      if (!validDropInterval(intervalMs)) {
        throw new RangeError("Flaggo returned an interval outside the contract bounds.");
      }
      options.experience.registerDecision(attributes, intervalMs);
      const evaluation = response.value.evaluation;
      const selection: DropIntervalSelection = {
        intervalMs,
        exposureId: attributes.exposure_id,
        contractDigest: response.value.contractDigest,
        executableDigest: response.value.executableDigest,
        source: "flaggo",
        status: evaluation.source === "rule" ? `rule ${evaluation.rule}` : "contract default",
      };
      span?.setAttributes({
        "tetris.drop_interval.ms": intervalMs,
        "tetris.drop_interval.source": selection.source,
      });
      options.telemetry?.logger.emit({
        eventName: "tetris.drop_interval.selected",
        attributes: {
          "tetris.session.id": attributes.session_id,
          "tetris.exposure.id": attributes.exposure_id,
          "tetris.drop_interval.ms": intervalMs,
          "tetris.drop_interval.source": selection.source,
        },
      });
      return selection;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      span?.recordException(error instanceof Error ? error : message);
      span?.setStatus({ code: SpanStatusCode.ERROR, message });
      options.telemetry?.logger.emit({
        eventName: "tetris.drop_interval.selected",
        attributes: {
          "tetris.session.id": attributes.session_id,
          "tetris.drop_interval.source": "local-fallback",
          "error.message": message,
        },
      });
      if (signal?.aborted === true) throw error;
      return {
        intervalMs: attributes.current_drop_interval_ms,
        source: "local-fallback",
        status: `Flaggo unavailable: ${message}`,
      };
    } finally {
      span?.end();
    }
  };

  return {
    async select(context, signal) {
      options.experience.observe(options.state(), options.now());
      const input = options.experience.runtimeInput(context);
      return options.telemetry === undefined ? select(input, signal)
        : options.telemetry.tracer.startActiveSpan("tetris.drop_interval.select", {
          attributes: {
            "tetris.session.id": input.session_id,
            "tetris.exposure.id": input.exposure_id,
          },
        }, (span) => select(input, signal, span));
    },
  };
}
