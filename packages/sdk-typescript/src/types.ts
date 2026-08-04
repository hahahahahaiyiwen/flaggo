export type DecisionValue = boolean | number | string;
export type RuntimeContextValue = DecisionValue | null;
export type Sha256Digest = `sha256:${string}`;

export interface SignalRef {
  key: string;
}

export interface SignalInput {
  signal: SignalRef;
  value: RuntimeContextValue;
}

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

interface DecisionDefinitionCommon {
  definitionId?: string;
  owner?: string;
  key: string;
  runtimeContextSchema?: Record<
    string,
    {
      type: "boolean" | "number" | "string";
      required?: boolean;
      target?: string;
    }
  >;
  targetHierarchy?: string[];
  signals?: {
    allowed?: SignalRef[];
    evidence?: SignalRef[];
    guardrails?: SignalRef[];
  };
  inference?: {
    target: string;
    inputs?: SignalRef[];
    fallbackOrder?: string[];
  };
  intent?: Record<string, unknown>;
  onlineStrategy?: Record<string, unknown>;
  policy: DecisionPolicy;
  requestedApproval?: "automatic" | "human" | "policy-default";
  revision?: string;
  contractDigest?: Sha256Digest;
  schemaDigest?: Sha256Digest;
}

export interface BooleanDecisionDefinition extends DecisionDefinitionCommon {
  valueType: "boolean";
  actionSpace: BooleanActionSpace;
  fallback: {
    value: boolean;
    reason?: string;
  };
}

export interface NumberDecisionDefinition extends DecisionDefinitionCommon {
  valueType: "number";
  actionSpace: NumberActionSpace;
  fallback: {
    value: number;
    reason?: string;
  };
}

export interface StringDecisionDefinition extends DecisionDefinitionCommon {
  valueType: "string";
  actionSpace: StringActionSpace;
  fallback: {
    value: string;
    reason?: string;
  };
}

export type DecisionDefinition =
  | BooleanDecisionDefinition
  | NumberDecisionDefinition
  | StringDecisionDefinition;

interface SignalDeclarationBase {
  key: string;
  schemaDigest?: Sha256Digest;
}

export interface EventSignalDeclaration extends SignalDeclarationBase {
  kind: "event";
  fields: Record<string, "boolean" | "number" | "string">;
  units?: Record<string, string>;
}

export interface AppEmittedMetricSignalDeclaration
  extends SignalDeclarationBase {
  kind: "metric";
  type: "boolean" | "number" | "string";
  source: "app-emitted";
  unit?: string;
  range?: [number, number];
}

export interface DerivedMetricSignalDeclaration extends SignalDeclarationBase {
  kind: "metric";
  type: "boolean" | "number" | "string";
  source: "derived";
  unit?: string;
  from: SignalRef[];
  aggregation: string;
  window: string;
  range?: [number, number];
}

export type SignalDeclaration =
  | EventSignalDeclaration
  | AppEmittedMetricSignalDeclaration
  | DerivedMetricSignalDeclaration;

export interface DecisionDefinitionBundle {
  format: "flaggo.decision-definition-bundle/v1";
  application: {
    id: string;
    environment: string;
  };
  build?: {
    buildId?: string;
    artifactDigest?: string;
    version?: string;
  };
  source: {
    repository?: string;
    path?: string;
    commit?: string;
  };
  signals?: SignalDeclaration[];
  definitions: DecisionDefinition[];
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
  signalKey?: string;
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
  confidence: ConfidenceReport;
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
  definition?: NumberDecisionDefinition;
  context: Record<string, RuntimeContextValue>;
  runtimeTarget?: DecisionTargetRef;
  inputs?: SignalInput[];
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
