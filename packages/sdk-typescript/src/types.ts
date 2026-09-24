export type DecisionValue = boolean | number | string;
export type RuntimeContextValue = DecisionValue | null;
export type Sha256Digest = `sha256:${string}`;

export interface DecisionTargetRef {
  type: string;
  id: string;
}

export interface NumberActionSpace {
  type: "number";
  min: number;
  max: number;
  step?: number;
  default: number;
}

export interface BooleanActionSpace {
  type: "boolean";
  default: boolean;
}

export interface StringActionSpace {
  type: "string";
  allowedValues?: string[];
  default: string;
}

export type PolicyConstraint =
  | { kind: "number-bounds"; min: number; max: number }
  | { kind: "max-delta"; value: number }
  | { kind: "cooldown"; seconds: number }
  | { kind: "min-evidence-quality"; value: number }
  | { kind: "max-model-uncertainty"; value: number }
  | { kind: "min-expected-outcome"; value: number }
  | { kind: "min-sample-size"; value: number }
  | { kind: "pause"; paused: boolean };

export interface ReferencePolicy {
  kind: "reference";
  policyId: string;
}

export interface InlinePolicy {
  kind: "inline";
  constraints: PolicyConstraint[];
  clientFallback?: {
    requiredEvidenceUnavailable: "allow" | "forbid";
  };
}

export type DecisionPolicy = ReferencePolicy | InlinePolicy;

export type PrimitiveType = "boolean" | "number" | "string";

export type InputSchema =
  | { type: "number"; unit?: string; range?: readonly [number, number] }
  | { type: "boolean" | "string" };

export type RequestInput = InputSchema & {
  source: "request";
  meaning: string;
};

export interface EvidenceInput {
  source: "evidence";
  binding: string;
}

export type DecisionInput = RequestInput | EvidenceInput;

export interface ContextField {
  type: PrimitiveType;
  required?: boolean;
  target?: string;
}

export interface AttributeSelector {
  from: "resourceAttributes" | "attributes" | "eventAttributes";
  key: string;
}

export interface TelemetryScope {
  name: string;
  version?: string;
}

interface TelemetrySelector {
  scope: TelemetryScope;
  resourceAttributes: Readonly<Record<string, DecisionValue>>;
  attributes?: Readonly<Record<string, DecisionValue>>;
}

export type TelemetrySource = TelemetrySelector & (
  | {
      kind: "metric";
      name: string;
      dataType: "gauge";
      value: { from: "value" };
    }
  | {
      kind: "span";
      name: string;
      value: { from: "duration" } | { from: "attributes"; key: string };
    }
  | {
      kind: "span";
      name: string;
      eventName: string;
      eventAttributes?: Readonly<Record<string, DecisionValue>>;
      value: { from: "eventAttributes"; key: string };
    }
  | ({
      kind: "log";
      value: { from: "attributes"; key: string } | { from: "body"; path: readonly string[] };
    } & (
      | { eventName: string; bodyEquals?: never }
      | { eventName?: never; bodyEquals: DecisionValue }
    ))
);

export type EvidenceBinding = InputSchema & {
  meaning: string;
  source: TelemetrySource;
  projection: { kind: "latest" };
  target:
    | { type: "global" }
    | { type: string; idAttribute: AttributeSelector };
  freshness: { maxAgeSeconds: number };
  sampling: { accept: "observed" };
  attribution:
    | { kind: "none" }
    | { kind: "confirmed-exposure"; exposureIdAttribute: AttributeSelector };
};

export type NumericObjective =
  | { evidence: string; direction: "minimize" | "maximize" }
  | { evidence: string; direction: "target"; target: number };

export type DecisionIntent =
  | { type: "natural-language"; text: string }
  | {
      type: "numeric-objective";
      primary: NumericObjective;
      secondary?: readonly NumericObjective[];
      rationale?: string;
    };

export interface DecisionDefinition {
  result: NumberActionSpace | BooleanActionSpace | StringActionSpace;
  context?: Readonly<Record<string, ContextField>>;
  targeting: {
    hierarchy: readonly string[];
    primary: string;
    fallbackOrder: readonly string[];
  };
  inputs?: Readonly<Record<string, DecisionInput>>;
  evidence?: Readonly<Record<string, EvidenceBinding>>;
  intent?: DecisionIntent;
  policy: DecisionPolicy;
  owner?: string;
}

export interface NumberDecisionDefinition extends DecisionDefinition {
  result: NumberActionSpace;
}

export interface DecisionDefinitionBundle {
  format: "flaggo.decision-definition-bundle/v2";
  application: {
    id: string;
    environment: string;
  };
  build?: {
    buildId?: string;
    artifactDigest?: string;
    version?: string;
  };
  source?: {
    repository?: string;
    path?: string;
    commit?: string;
  };
  decisions: Readonly<Record<string, DecisionDefinition>>;
}

export interface CatalogDecision {
  result: DecisionDefinition["result"];
  context: Readonly<Record<string, ContextField>>;
  inputs: Readonly<Record<string, DecisionInput>>;
  contractDigest: Sha256Digest;
}

export interface RuntimeCatalog {
  format: "flaggo.runtime-catalog/v1";
  application: { id: string; environment: string };
  bundleDigest: Sha256Digest;
  decisions: Readonly<Record<string, CatalogDecision>>;
}

export interface AcceptedDefinition {
  definitionId: string;
  revision: string;
  contractDigest: Sha256Digest;
}

export interface RegistrationReceipt {
  application: string;
  environment: string;
  bundleDigest: Sha256Digest;
  buildId?: string;
  artifactDigest?: string;
  acceptedDefinitions: Record<string, AcceptedDefinition>;
  compatibility: "identical" | "metadata-only" | "new-contract-required";
  status: "approved";
  changes?: unknown[];
  issues: ContractIssue[];
}

export interface ContractIssue {
  code: string;
  severity: "error" | "warning";
  path: string;
  message: string;
  decisionKey?: string;
  inputKey?: string;
}

export interface RequiresApprovalResult {
  status: "requires-approval";
  approvalRequestId: string;
  application: string;
  environment: string;
  bundleDigest: Sha256Digest;
  compatibility: "new-contract-required";
  expiresAt: string;
  snapshotUrl: string;
  supersedesApprovalRequestId?: string;
  changes: unknown[];
  issues: ContractIssue[];
}

export interface RuntimeContractIdentity extends AcceptedDefinition {
  bundleDigest?: Sha256Digest;
  buildId?: string;
  deploymentId?: string;
  artifactDigest?: string;
}

export interface ExposureDirectiveRequired {
  confirmationRequired: true;
  confirmToken: string;
}

export interface ExposureDirectiveNotRequired {
  confirmationRequired: false;
}

export type ExposureDirective =
  | ExposureDirectiveRequired
  | ExposureDirectiveNotRequired;

export interface TargetResolutionProvenance {
  targetType: string;
  claimedId?: string;
  resolvedId: string;
  source:
    | "client-claimed"
    | "client-verified"
    | "server-derived"
    | "server-replaced";
}

export interface ConfidenceReport {
  evidenceQuality: number;
  modelUncertainty?: number;
  expectedOutcome?: number;
}

export interface PolicyEvaluationResult<
  TResult extends "approved" | "blocked" | "fallback" =
    | "approved"
    | "blocked"
    | "fallback",
> {
  result: TResult;
  reasons: string[];
  appliedConstraints: string[];
  clientFallback?: {
    requiredEvidenceUnavailable: "allow" | "forbid";
  };
}

export interface ContractRuntimeStatus extends AcceptedDefinition {
  bundleDigest?: Sha256Digest;
  buildId?: string;
  deploymentId?: string;
  integrity: "verified";
  compatibility?: "identical" | "metadata-only" | "new-contract-required";
}

type DecisionValuePayload<T extends DecisionValue> =
  T extends boolean
    ? { valueType: "boolean"; value: T }
    : T extends number
      ? { valueType: "number"; value: T }
      : T extends string
        ? { valueType: "string"; value: T }
        : never;

interface ServerDecisionCommon {
  decisionKey: string;
  definition: {
    appId: string;
    environment: string;
    key: string;
    definitionId: string;
    revision: string;
  };
  decisionId: string;
  runtimeTarget?: DecisionTargetRef;
  controlTarget?: DecisionTargetRef;
  targetProvenance: TargetResolutionProvenance[];
  resolutionChain: string[];
  definitionStatus: ContractRuntimeStatus;
  exposure: ExposureDirective;
  reason: string;
  auditId: string;
}

interface ActiveValueDecision {
  decisionMode: "active-value";
  strategyId?: never;
  confidence: ConfidenceReport | null;
  fallback: {
    source: "server";
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: false;
    reason: string | null;
  };
  policy: PolicyEvaluationResult<"approved">;
}

interface StrategyDecision {
  decisionMode: "strategy" | "experiment";
  strategyId: string;
  confidence: ConfidenceReport | null;
  fallback: {
    source: "server";
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: false;
    reason: string | null;
  };
  policy: PolicyEvaluationResult<"approved">;
}

interface ServerFallbackDecision {
  decisionMode: "fallback";
  strategyId?: never;
  confidence: null;
  fallback: {
    source: "server";
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: true;
    reason: string | null;
  };
  policy: PolicyEvaluationResult<"blocked" | "fallback">;
}

export type ActiveValueDecisionResult<
  T extends DecisionValue = DecisionValue,
> = {
  source: "server";
} & ServerDecisionCommon & DecisionValuePayload<T> & ActiveValueDecision;

export type StrategyDecisionResult<
  T extends DecisionValue = DecisionValue,
> = {
  source: "server";
} & ServerDecisionCommon & DecisionValuePayload<T> & StrategyDecision;

export type ServerFallbackDecisionResult<
  T extends DecisionValue = DecisionValue,
> = {
  source: "server";
} & ServerDecisionCommon & DecisionValuePayload<T> & ServerFallbackDecision;

export type ServerDecisionResult<T extends DecisionValue = DecisionValue> =
  | ActiveValueDecisionResult<T>
  | StrategyDecisionResult<T>
  | ServerFallbackDecisionResult<T>;

type WithoutSource<T> = T extends unknown ? Omit<T, "source"> : never;

export type ServerDecisionPayload<T extends DecisionValue = DecisionValue> =
  WithoutSource<ServerDecisionResult<T>>;

export type ClientFallbackResult<T extends DecisionValue = DecisionValue> = {
  source: "client-fallback";
  decisionKey: string;
  expectedContract: RuntimeContractIdentity;
  decisionMode: "fallback";
  confidence: null;
  reason: string;
  fallback: {
    source: "client-fallback";
    resolutionFallbackUsed: false;
    decisionFallbackUsed: true;
    reason: string;
  };
} & DecisionValuePayload<T>;

export type DecisionResult<T extends DecisionValue = DecisionValue> =
  | ServerDecisionResult<T>
  | ClientFallbackResult<T>;

export type DecisionReceipt<T extends DecisionValue = DecisionValue> =
  | {
      source: "server";
      value: T;
      decisionId: string;
      exposure: ExposureDirective;
    }
  | {
      source: "client-fallback";
      value: T;
      expectedContract: RuntimeContractIdentity;
      reason: string;
    };

export interface NumberTuneRequest {
  context?: Record<string, RuntimeContextValue>;
  runtimeTarget?: DecisionTargetRef;
  inputs?: Record<string, DecisionValue>;
  idempotencyKey?: string;
  correlationId?: string;
}

export interface ExposureConfirmationResult {
  exposureId: string;
  decisionId: string;
  status: "confirmed";
  confirmedAt: string;
}

export interface ProblemDetails {
  type: string;
  title?: string;
  status: number;
  detail?: string;
  instance?: string;
  code: string;
  correlationId?: string;
  clientFallback?: {
    eligible: boolean;
    reason?: string;
  };
  issues?: ContractIssue[];
  retryAfterSeconds?: number;
}

export type FetchLike = (
  input: string | URL | globalThis.Request,
  init?: RequestInit,
) => Promise<Response>;
