export { createDecisionClient } from "./client.js";
export { SDK_VERSION } from "../generated/package-version.generated.js";
export {
  defineDecisionBindings,
  type CurrentExposure,
  type DecisionBinding,
  type DecisionBindings,
  type DecisionCatalog,
  type DecisionClient,
  type DecisionClientConfiguration,
  type DecisionRequest,
  type DecisionSpec,
  type RuntimeDecision,
  type RuntimeEvaluation,
} from "./types.js";
export {
  createFlaggoOtlpLogger,
  type FlaggoOtlpEvent,
  type FlaggoOtlpJsonLogs,
  type FlaggoOtlpLogger,
  type FlaggoOtlpLoggerConfiguration,
} from "./otlp-logs.js";
export {
  createFlaggoTelemetry,
  recordDecisionReceived,
  recordOutcome,
  type CorrelationAttributes,
  type DecisionReceivedTelemetryInput,
  type DecisionTelemetryContext,
  type FlaggoTelemetry,
  type FlaggoTelemetryConfiguration,
  type FlaggoTelemetryLogger,
  type OutcomeTelemetryInput,
} from "./telemetry.js";
export {
  FlaggoAbortError,
  FlaggoError,
  FlaggoHttpError,
  FlaggoTimeoutError,
  FlaggoTransportError,
  InvalidFlaggoInputError,
  InvalidServerResponseError,
  MissingDecisionBindingError,
} from "../errors.js";
export type {
  CredentialProvider,
  FetchLike,
  FlaggoResponse,
  FlaggoResponseMetadata,
  JsonObject,
  JsonValue,
  ProblemDetails,
  RequestOptions,
  RetryPolicy,
  Sha256Digest,
} from "../shared/types.js";
