export type Sha256Digest = `sha256:${string}`;

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };

export type ValueType =
  | "null"
  | "boolean"
  | "integer"
  | "number"
  | "string"
  | "array"
  | "object";

export interface ValueSchema {
  type: ValueType;
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
  multipleOf?: number;
  minLength?: number;
  maxLength?: number;
  items?: ValueSchema;
  minItems?: number;
  maxItems?: number;
  uniqueItems?: boolean;
  properties?: Readonly<Record<string, ValueSchema>>;
  required?: readonly string[];
  additionalProperties?: boolean;
  minProperties?: number;
  maxProperties?: number;
  description?: string;
}

export interface ContractAttribute {
  name: string;
  schema: ValueSchema;
}

export interface ContractResult<TResult extends JsonValue = JsonValue> {
  schema: ValueSchema;
  default: TResult;
}

export type RuleWhen =
  | { condition: string }
  | { expression: string };

export type RuleReturn<TResult extends JsonValue = JsonValue> =
  | { value: TResult }
  | { expression: string };

export interface AuthoredRule<TResult extends JsonValue = JsonValue> {
  name: string;
  description?: string;
  when: RuleWhen;
  return: RuleReturn<TResult>;
}

export interface AuthoredExecutable<TResult extends JsonValue = JsonValue> {
  rules: readonly AuthoredRule<TResult>[];
}

export interface LearningPolicy {
  mode: "auto-activation";
  evaluate: {
    interval: string;
  };
}

export interface EvidenceDefinition {
  name: string;
  description?: string;
  attribute: string;
  binding: string;
  correlateBy: readonly string[];
}

export interface LearningObjective {
  primary: {
    evidence: string;
    direction: "minimize" | "maximize";
  };
  guardrails?: readonly {
    name: string;
    description?: string;
    expression: string;
  }[];
}

export interface LearningDefinition {
  policy: LearningPolicy;
  evidence: readonly EvidenceDefinition[];
  objective: LearningObjective;
}

export interface DecisionContract<TResult extends JsonValue = JsonValue> {
  name: string;
  expression_syntax: "flaggo.cel/v1";
  attributes: readonly ContractAttribute[];
  result: ContractResult<TResult>;
  authoredExecutable?: AuthoredExecutable<TResult>;
  learning?: LearningDefinition;
}

export interface ValidationIssue {
  code: string;
  severity: "error" | "warning";
  path: string;
  message: string;
}

export type DecisionContractValidationResult =
  | {
      status: "valid";
      contractDigest: Sha256Digest;
      issues: readonly ValidationIssue[];
    }
  | {
      status: "invalid";
      issues: readonly ValidationIssue[];
    };

export interface DecisionContractVersion<TResult extends JsonValue = JsonValue> {
  name: string;
  contractDigest: Sha256Digest;
  status: "ready";
  acceptedAt: string;
  activeExecutableDigest: Sha256Digest;
  contract: DecisionContract<TResult>;
}

export interface DecisionContractVersionSummary {
  contractDigest: Sha256Digest;
  status: "ready";
  acceptedAt: string;
  activeExecutableDigest: Sha256Digest;
}

export interface DecisionContractVersionList {
  name: string;
  currentContractDigest: Sha256Digest;
  versions: readonly DecisionContractVersionSummary[];
  nextCursor: string | null;
}

export interface RuntimeContractBinding {
  contractDigest: Sha256Digest;
}

export type RuntimeContractBindings =
  Readonly<Record<string, RuntimeContractBinding>>;

export interface CurrentExposure {
  exposureId: string;
}

export interface RuntimeDecisionRequest {
  attributes?: Readonly<Record<string, JsonValue>>;
  currentExposure?: CurrentExposure;
  correlationId?: string;
}

export interface RuntimeInput {
  attributes: Readonly<Record<string, JsonValue>>;
  currentExposure?: CurrentExposure;
}

export type RuntimeEvaluation =
  | { source: "default" }
  | { source: "rule"; rule: string };

export interface RuntimeDecision<TResult extends JsonValue = JsonValue> {
  contractDigest: Sha256Digest;
  executableDigest: Sha256Digest;
  result: TResult;
  evaluation: RuntimeEvaluation;
}

export interface ProblemDetails {
  type: string;
  title?: string;
  status: number;
  detail?: string;
  instance?: string;
}

export interface FlaggoResponseMetadata {
  correlationId?: string;
  retryAfterSeconds?: number;
}

export type FetchLike = (
  input: string | URL | globalThis.Request,
  init?: RequestInit,
) => Promise<Response>;
