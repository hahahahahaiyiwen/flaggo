/*
 * Generated from Flaggo JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */

/**
 * Stable user-owned identity of a named DecisionContract resource.
 *
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "DecisionName".
 */
export type DecisionName = string;
/**
 * Project digest form: sha256:<lowercase-hex>.
 *
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "Sha256Digest".
 */
export type Sha256Digest = string;
/**
 * RFC 3339 UTC timestamp using the Z suffix.
 *
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "Rfc3339".
 */
export type Rfc3339 = string;
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "JsonValue".
 *
 * This interface was referenced by `RuntimeAttributes`'s JSON-Schema definition
 * via the `patternProperty` "^[A-Za-z][A-Za-z0-9_]*$".
 */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | {
      [k: string]: JsonValue;
    };
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "EvaluationProvenance".
 */
export type EvaluationProvenance = RuleEvaluation | DefaultEvaluation;

/**
 * Strict wire models for stateless DecisionContract evaluation and service health. The SDK constructs the complete RuntimeInput, including internal attributes and optional current exposure.
 */
export interface FlaggoRuntimeAPIModelsV3 {
  [k: string]: unknown;
}
/**
 * Complete SDK-constructed attribute map. The v3 wire profile reserves underscore-prefixed names and defines only _random. Semantic validation rejects user attributes not declared by the contract.
 *
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "RuntimeAttributes".
 */
export interface RuntimeAttributes {
  /**
   * SDK-generated internal random value, reused for retries of one logical evaluation.
   */
  _random: number;
  [k: string]: JsonValue | number;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "CurrentExposure".
 */
export interface CurrentExposure {
  /**
   * SDK-managed identity of the previous decision actually applied in the current activity context.
   */
  exposureId: string;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "RuntimeInput".
 */
export interface RuntimeInput {
  attributes: RuntimeAttributes;
  currentExposure?: CurrentExposure;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "RuleEvaluation".
 */
export interface RuleEvaluation {
  source: "rule";
  rule: string;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "DefaultEvaluation".
 */
export interface DefaultEvaluation {
  source: "default";
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "RuntimeDecision".
 */
export interface RuntimeDecision {
  contractDigest: Sha256Digest;
  executableDigest: Sha256Digest;
  /**
   * Decision result. Runtime also validates it against the accepted DecisionContract result schema.
   */
  result:
    | null
    | boolean
    | number
    | string
    | JsonValue[]
    | {
        [k: string]: JsonValue;
      };
  evaluation: EvaluationProvenance;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "LivenessResult".
 */
export interface LivenessResult {
  status: "live";
  service: string;
  version: string;
  observedAt: Rfc3339;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "ReadinessCheck".
 */
export interface ReadinessCheck {
  name: string;
  status: "up" | "degraded" | "down";
  required: boolean;
}
/**
 * This interface was referenced by `FlaggoRuntimeAPIModelsV3`'s JSON-Schema
 * via the `definition` "ReadinessResult".
 */
export interface ReadinessResult {
  status: "ready" | "degraded" | "not-ready";
  observedAt: Rfc3339;
  /**
   * @minItems 1
   */
  checks: ReadinessCheck[];
}
