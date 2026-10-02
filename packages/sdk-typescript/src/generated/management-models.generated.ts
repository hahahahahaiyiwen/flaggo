/*
 * Generated from Flaggo v3 JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */

/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Sha256Digest".
 */
export type Sha256Digest = string;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionName".
 */
export type DecisionName = string;
/**
 * A user-declared attribute name. Names beginning with '_' are reserved for Flaggo internal attributes.
 *
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "AttributeName".
 */
export type AttributeName = string;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "MemberName".
 */
export type MemberName = string;
/**
 * An expression in the DecisionContract's expression_syntax.
 *
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Expression".
 */
export type Expression = string;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "JsonValue".
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
 * A JSON Schema 2020-12 document in the bounded flaggo.value-schema/v1 profile.
 *
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ValueSchema".
 */
export type ValueSchema =
  | NullValueSchema
  | BooleanValueSchema
  | NumberValueSchema
  | IntegerValueSchema
  | StringValueSchema
  | ArrayValueSchema
  | ObjectValueSchema;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "AuthoredWhen".
 */
export type AuthoredWhen = NaturalLanguageWhen | ExpressionWhen;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "RuleReturn".
 */
export type RuleReturn = LiteralReturn | ExpressionReturn;
/**
 * The one application-owned OpenTelemetry signal selected for this evidence declaration.
 *
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "EvidenceSource".
 */
export type EvidenceSource =
  MetricEvidenceSource | LogEvidenceSource | SpanEvidenceSource | SpanEventEvidenceSource;
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "WarningIssue".
 */
export type WarningIssue = ValidationIssue & {
  severity?: "warning";
};
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ErrorIssue".
 */
export type ErrorIssue = ValidationIssue & {
  severity?: "error";
};
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionContractValidationResult".
 */
export type DecisionContractValidationResult = ValidDecisionContract | InvalidDecisionContract;

/**
 * DecisionContract acceptance and status models for the Flaggo Management API v3.
 */
export interface FlaggoManagementModelsV3 {}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "NullValueSchema".
 */
export interface NullValueSchema {
  type: "null";
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "BooleanValueSchema".
 */
export interface BooleanValueSchema {
  type: "boolean";
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "NumberValueSchema".
 */
export interface NumberValueSchema {
  type: "number";
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
  multipleOf?: number;
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "IntegerValueSchema".
 */
export interface IntegerValueSchema {
  type: "integer";
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
  multipleOf?: number;
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "StringValueSchema".
 */
export interface StringValueSchema {
  type: "string";
  minLength?: number;
  maxLength?: number;
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ArrayValueSchema".
 */
export interface ArrayValueSchema {
  type: "array";
  items: ValueSchema;
  minItems?: number;
  maxItems?: number;
  uniqueItems?: boolean;
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ObjectValueSchema".
 */
export interface ObjectValueSchema {
  type: "object";
  properties: {
    [k: string]: ValueSchema;
  };
  /**
   * Required property names form an unordered set; order is non-semantic.
   *
   * @maxItems 256
   */
  required?: string[];
  additionalProperties: false;
  minProperties?: number;
  maxProperties?: number;
  description?: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Attribute".
 */
export interface Attribute {
  name: AttributeName;
  schema: ValueSchema;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Result".
 */
export interface Result {
  schema: ValueSchema;
  /**
   * Literal fallback result. Contract Acceptance verifies that it satisfies schema.
   */
  default:
    | null
    | boolean
    | number
    | string
    | JsonValue[]
    | {
        [k: string]: JsonValue;
      };
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "NaturalLanguageWhen".
 */
export interface NaturalLanguageWhen {
  condition: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ExpressionWhen".
 */
export interface ExpressionWhen {
  expression: Expression;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "LiteralReturn".
 */
export interface LiteralReturn {
  value: JsonValue;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ExpressionReturn".
 */
export interface ExpressionReturn {
  expression: Expression;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "AuthoredRule".
 */
export interface AuthoredRule {
  name: MemberName;
  description?: string;
  when: AuthoredWhen;
  return: RuleReturn;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "AuthoredExecutable".
 */
export interface AuthoredExecutable {
  /**
   * Ordered first-match-wins rules. Rule names must be unique.
   *
   * @minItems 1
   */
  rules: AuthoredRule[];
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "LearningEvaluationPolicy".
 */
export interface LearningEvaluationPolicy {
  /**
   * A positive ISO 8601 duration expressing the minimum delay between completed analysis attempts.
   */
  interval: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "LearningPolicy".
 */
export interface LearningPolicy {
  /**
   * Automatically attempt atomic activation after an evidence-generated candidate passes validation and a current-contract check.
   */
  mode: "auto-activation";
  evaluate: LearningEvaluationPolicy;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "EvidenceCorrelationAttribute".
 */
export interface EvidenceCorrelationAttribute {
  /**
   * The OTLP envelope level containing the correlation attribute. parentSpan is valid only for spanEvent sources.
   */
  location: "resource" | "scope" | "signal" | "parentSpan";
  /**
   * Exact case-sensitive OpenTelemetry attribute key.
   */
  attribute: string;
}
/**
 * Maps every correlateBy contract attribute to one exact OpenTelemetry attribute location and key.
 *
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "EvidenceCorrelationMap".
 */
export interface EvidenceCorrelationMap {
  [k: string]: EvidenceCorrelationAttribute;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "MetricEvidenceSource".
 */
export interface MetricEvidenceSource {
  kind: "metric";
  /**
   * Exact case-sensitive InstrumentationScope name.
   */
  scope: string;
  /**
   * Exact case-sensitive metric name.
   */
  name: string;
  metricKind: "gauge" | "sum" | "histogram" | "exponentialHistogram" | "summary";
  /**
   * Exact case-sensitive metric unit. Use an empty string for a metric with no unit.
   */
  unit: string;
  correlation: EvidenceCorrelationMap;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "LogEvidenceSource".
 */
export interface LogEvidenceSource {
  kind: "log";
  /**
   * Exact case-sensitive InstrumentationScope name.
   */
  scope: string;
  /**
   * Exact case-sensitive LogRecord event name.
   */
  name: string;
  correlation: EvidenceCorrelationMap;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "SpanEvidenceSource".
 */
export interface SpanEvidenceSource {
  kind: "span";
  /**
   * Exact case-sensitive InstrumentationScope name.
   */
  scope: string;
  /**
   * Exact case-sensitive span name.
   */
  name: string;
  correlation: EvidenceCorrelationMap;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "SpanEventEvidenceSource".
 */
export interface SpanEventEvidenceSource {
  kind: "spanEvent";
  /**
   * Exact case-sensitive InstrumentationScope name.
   */
  scope: string;
  /**
   * Exact case-sensitive parent span name.
   */
  spanName: string;
  /**
   * Exact case-sensitive span-event name.
   */
  name: string;
  correlation: EvidenceCorrelationMap;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Evidence".
 */
export interface Evidence {
  name: MemberName;
  description?: string;
  /**
   * A user-declared attribute name. Names beginning with '_' are reserved for Flaggo internal attributes.
   */
  attribute: string;
  /**
   * Unordered set of contract attributes whose OpenTelemetry locations are declared by source.correlation.
   *
   * @maxItems 128
   */
  correlateBy: AttributeName[];
  source: EvidenceSource;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "PrimaryObjective".
 */
export interface PrimaryObjective {
  evidence: MemberName;
  direction: "minimize" | "maximize";
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Guardrail".
 */
export interface Guardrail {
  name: MemberName;
  description?: string;
  expression: Expression;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "LearningObjective".
 */
export interface LearningObjective {
  primary: PrimaryObjective;
  /**
   * Guardrail names must be unique; order is non-semantic.
   */
  guardrails?: Guardrail[];
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "Learning".
 */
export interface Learning {
  policy: LearningPolicy;
  /**
   * One logical observed value per entry. Evidence names must be unique; order is non-semantic.
   *
   * @minItems 1
   */
  evidence: Evidence[];
  objective: LearningObjective;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionContract".
 */
export interface DecisionContract {
  name: DecisionName;
  expression_syntax: "flaggo.cel/v1";
  /**
   * User-declared runtime attributes. Attribute names must be unique; order is non-semantic.
   *
   * @maxItems 128
   */
  attributes: Attribute[];
  result: Result;
  authoredExecutable?: AuthoredExecutable;
  learning?: Learning;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ExecutableRule".
 */
export interface ExecutableRule {
  name: MemberName;
  when: ExpressionWhen;
  return: RuleReturn;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionExecutable".
 */
export interface DecisionExecutable {
  contractDigest: Sha256Digest;
  kind: "rules";
  /**
   * Ordered deterministic rules. An empty array is the default executable.
   */
  rules: ExecutableRule[];
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ValidationIssue".
 */
export interface ValidationIssue {
  code: string;
  severity: "error" | "warning";
  /**
   * JSON Pointer to the affected contract location; an empty string identifies the root.
   */
  path: string;
  message: string;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "ValidDecisionContract".
 */
export interface ValidDecisionContract {
  status: "valid";
  contractDigest: Sha256Digest;
  issues: WarningIssue[];
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "InvalidDecisionContract".
 */
export interface InvalidDecisionContract {
  status: "invalid";
  /**
   * @minItems 1
   */
  issues: ValidationIssue[];
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionContractVersionSummary".
 */
export interface DecisionContractVersionSummary {
  contractDigest: Sha256Digest;
  /**
   * A version is deployed only after its generated default executable has been activated.
   */
  status: "ready";
  acceptedAt: string;
  activeExecutableDigest: Sha256Digest;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionContractVersion".
 */
export interface DecisionContractVersion {
  name: DecisionName;
  contractDigest: Sha256Digest;
  /**
   * A version is deployed only after its generated default executable has been activated.
   */
  status: "ready";
  acceptedAt: string;
  activeExecutableDigest: Sha256Digest;
  contract: DecisionContract1;
}
/**
 * The immutable accepted contract document. Its name must equal the enclosing resource name.
 */
export interface DecisionContract1 {
  name: DecisionName;
  expression_syntax: "flaggo.cel/v1";
  /**
   * User-declared runtime attributes. Attribute names must be unique; order is non-semantic.
   *
   * @maxItems 128
   */
  attributes: Attribute[];
  result: Result;
  authoredExecutable?: AuthoredExecutable;
  learning?: Learning;
}
/**
 * This interface was referenced by `FlaggoManagementModelsV3`'s JSON-Schema
 * via the `definition` "DecisionContractVersionList".
 */
export interface DecisionContractVersionList {
  name: DecisionName;
  currentContractDigest: Sha256Digest;
  /**
   * Versions ordered by acceptedAt descending and then contractDigest descending.
   */
  versions: DecisionContractVersionSummary[];
  /**
   * Opaque cursor for the next page, or null when this is the final page.
   */
  nextCursor: string | null;
}
