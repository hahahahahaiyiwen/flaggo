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

export interface NumberDecisionDefinition {
  definitionId?: string;
  owner?: string;
  key: string;
  valueType: "number";
  actionSpace: NumberActionSpace;
  fallback: {
    value: number;
    reason?: string;
  };
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
  intent: Record<string, unknown>;
  onlineStrategy?: Record<string, unknown>;
  policy: {
    kind: "inline" | "reference";
    constraints?: Array<{ kind: string; [key: string]: unknown }>;
    clientFallback?: {
      requiredEvidenceUnavailable: "allow" | "forbid";
    };
    [key: string]: unknown;
  };
  requestedApproval?: "automatic" | "human" | "policy-default";
  revision?: string;
  contractDigest?: Sha256Digest;
  schemaDigest?: Sha256Digest;
}

export type DecisionDefinition = NumberDecisionDefinition;

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

export interface ServerDecisionResult<T extends DecisionValue = DecisionValue> {
  source: "server";
  decisionKey: string;
  definition: {
    appId: string;
    environment: string;
    key: string;
    definitionId: string;
    revision: string;
  };
  decisionId: string;
  value: T;
  valueType: "boolean" | "number" | "string";
  decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
  strategyId?: string;
  confidence: {
    evidenceQuality: number;
    modelUncertainty?: number;
    expectedOutcome?: number;
  } | null;
  runtimeTarget?: DecisionTargetRef;
  controlTarget?: DecisionTargetRef;
  targetProvenance: TargetResolutionProvenance[];
  resolutionChain: string[];
  fallback: {
    source: "server";
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: boolean;
    reason: string | null;
  };
  policy: {
    result: "approved" | "blocked" | "fallback";
    reasons: string[];
    appliedConstraints: string[];
  };
  definitionStatus: RuntimeContractIdentity & {
    integrity: "verified";
    compatibility?: "identical" | "metadata-only" | "new-contract-required";
  };
  exposure: ExposureDirective;
  reason: string;
  auditId: string;
}

export interface ClientFallbackResult<T extends DecisionValue = DecisionValue> {
  source: "client-fallback";
  decisionKey: string;
  expectedContract: RuntimeContractIdentity;
  decisionMode: "fallback";
  value: T;
  valueType: "boolean" | "number" | "string";
  confidence: null;
  reason: string;
  fallback: {
    source: "client-fallback";
    resolutionFallbackUsed: false;
    decisionFallbackUsed: true;
    reason: string;
  };
}

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
  definition: NumberDecisionDefinition;
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
