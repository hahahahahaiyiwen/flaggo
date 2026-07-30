# Shared Contracts Design

## Purpose

This document freezes the shared MVP contract shapes used across the Flaggo TypeScript SDK, Decision API, local adapters, tests, manifests, and future cloud adapters.

The goal is not to finalize every future field. The goal is to define a small, stable set of provider-neutral interfaces that can serve the Tetris MVP while leaving clear extension seams.

The mapping of these domain contracts to HTTP is currently a draft in the [Phase 1 API Contract Proposal](../API_CONTRACT_PROPOSAL.md). Its Phase 1 product decisions are accepted; wire-specific optionality and status behavior are not frozen until executable artifacts validate.

These contracts are open-source native:

- no cloud-provider-specific fields,
- no dependency on proprietary SDKs,
- JSON-serializable over REST where applicable,
- compatible with OpenAPI documentation,
- portable to local, container, and cloud-backed adapters.

## MVP contract rules

1. Runtime decision values are only `boolean`, `number`, or `string`.
2. Decision keys and definitions must be pre-registered or validated by manifest before production runtime.
3. Adaptive behavior is represented by a `DecisionStrategy`, not hidden application logic.
4. Online runtime returns a concrete value, even when that value came from a strategy.
5. Async intelligence produces proposals; governance activates values, strategies, experiments, holds, or fallback-only states; rollback is a transition that activates a replacement or previous state and marks the replaced state rolled back.
6. Runtime responses must include target, fallback, policy, and audit metadata.
7. New strategy types, storage backends, evidence sources, and policy rules must extend explicit interfaces instead of changing the runtime response shape.

## Primitive types

```ts
type ValueType = "boolean" | "number" | "string";
type DecisionValue = boolean | number | string;
type LifecycleState = "active" | "deprecated" | "retired";

type BuiltInTargetType = "session" | "user" | "cohort" | "global";
type TargetType = BuiltInTargetType | (string & {});

type DecisionTargetRef = {
  type: TargetType;
  id: string;
};

type DecisionDefinitionRef = {
  appId: string;
  environment: string;
  key: string;
  definitionId: string;
  revision: string;
};

type RuntimeContextValue = boolean | number | string | null;
type RuntimeContext = Record<string, RuntimeContextValue>;
```

Rules:

- `key` uses stable domain naming such as `tetris.dropInterval`.
- `environment` is provider-neutral, such as `dev`, `test`, `prod`.
- `DecisionTargetRef.type` is extensible, but MVP built-ins are `session`, `user`, `cohort`, and `global`.
- Runtime context must stay primitive and JSON-serializable for audit and policy evaluation.
- A signal `key` is its immutable semantic identity. Any schema or meaning change requires a new key; schema digests detect conflicting definitions under the same key.
- `schemaDigest` is generated from the canonical signal declaration with the digest field omitted. Tooling recomputes and verifies it when supplied; authors do not control it. Signal references contain only `key`, so presence or absence of digest metadata cannot change a decision-definition digest.
- A decision may only use signals referenced by explicit roles such as objectives, inference inputs, evidence, or guardrails. Tooling may materialize an associated-signal set in the extracted contract for governance, but authored definitions should not duplicate role references by hand.

## Action space

```ts
type ActionSpace =
  | NumberActionSpace
  | BooleanActionSpace
  | StringActionSpace;

type NumberActionSpace = {
  type: "number";
  min: number;
  max: number;
  step?: number;
  default: number;
};

type BooleanActionSpace = {
  type: "boolean";
  default: boolean;
};

type StringActionSpace = {
  type: "string";
  allowedValues?: string[];
  default: string;
};
```

Rules:

- `default` is the source of the static fallback unless a scoped fallback overrides it.
- Number values must be within `min` and `max`.
- Number values must align to `step` when `step` is present.
- String values should use `allowedValues` when the application expects a known set.

## Decision contract

```ts
type DecisionDefinition = {
  ref: DecisionDefinitionRef;
  lifecycle: LifecycleState;
  valueType: ValueType;
  actionSpace: ActionSpace;
  fallback: FallbackContract;
  runtimeContextSchema?: RuntimeContextSchema;
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: InferenceDeclaration;
  intent?: DecisionIntent;
  requestedApproval?: RequestedApprovalMode;
  policy: PolicyReference | InlinePolicy;
  onlineStrategy?: OnlineStrategyDeclaration;
  metadata?: Record<string, string>;
};

type FallbackContract = {
  value: DecisionValue;
  reason?: string;
};

type RuntimeContextSchema = Record<
  string,
  {
    type: "boolean" | "number" | "string";
    required?: boolean;
    target?: string;
  }
>;

type DecisionSignalReferences = {
  // Generated in registered contracts from the role references below plus intent and inference declarations.
  allowed?: SignalRef[];
  evidence?: SignalRef[];
  guardrails?: SignalRef[];
};

type SignalDeclaration =
  | EventSignalDeclaration
  | MetricSignalDeclaration;

type SignalRef = {
  key: string;
};

// Serialized as SignalRef; registry validation resolves and verifies the numeric metric declaration.
type NumericMetricRef = SignalRef;

type EventSignalDeclaration = {
  kind: "event";
  key: string;
  fields: Record<string, "boolean" | "number" | "string">;
  units?: Record<string, string>;
  schemaDigest?: string;
};

type MetricSignalDeclaration =
  | AppEmittedMetricSignalDeclaration
  | DerivedMetricSignalDeclaration;

type AppEmittedMetricSignalDeclaration = {
  kind: "metric";
  key: string;
  type: "boolean" | "number" | "string";
  source: "app-emitted";
  unit?: string;
  range?: [number, number];
  schemaDigest?: string;
};

type DerivedMetricSignalDeclaration = {
  kind: "metric";
  key: string;
  type: "boolean" | "number" | "string";
  source: "derived";
  unit?: string;
  from: SignalRef[];
  aggregation: string;
  window: string;
  range?: [number, number];
  schemaDigest?: string;
};

type InferenceDeclaration = {
  target: TargetType;
  inputs?: SignalRef[];
  fallbackOrder?: string[];
};

type DecisionIntent =
  | NaturalLanguageIntent
  | MetricObjectiveIntent;

type NaturalLanguageIntent = {
  type: "natural-language";
  text: string;
};

type MetricObjectiveIntent = {
  type: "metric-objective";
  primary: MetricObjective;
  secondary?: MetricObjective[];
  rationale?: string;
};

type MetricObjective =
  | {
      signal: NumericMetricRef;
      direction: "minimize" | "maximize";
    }
  | {
      signal: NumericMetricRef;
      direction: "target";
      target: number;
    };

type RequestedApprovalMode = "automatic" | "human" | "policy-default";

type PolicyReference = {
  kind: "reference";
  policyId: string;
};

type InlinePolicy = {
  kind: "inline";
  constraints: PolicyConstraint[];
};

type OnlineStrategyDeclaration = {
  mode: "active-value" | "approved-strategy" | "experiment" | "fallback-only";
  liveInputs?: string[];
};
```

MVP rule: `tetris.dropInterval` should use `onlineStrategy.mode = "approved-strategy"` and live inputs such as `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures`, and `currentLevel`.

`InferenceDeclaration.inputs` is structurally serialized as `SignalRef[]`, but every referenced key must resolve to an app-emitted primitive metric declaration. Events and service-derived metrics are invalid inference inputs. SDK type systems should enforce this before extraction; registry validation and the Decision API must enforce it again against registered signal declarations.

`MetricObjective.signal` is structurally serialized as a signal key, but it must resolve to a numeric metric declaration. The metric may be app-emitted or derived; events and boolean/string metrics are invalid objectives. SDKs should expose a branded numeric metric identity, and registry/API validation must enforce the same rule.

Metric objective direction is discriminated: `target` requires a finite numeric `target`, while `minimize` and `maximize` must not carry a `target` field. Canonical validation rejects both missing-target and unexpected-target forms.

`DecisionDefinition.policy` is required. Canonicalization never inserts an implicit environment or default policy when it is absent. Code-first shorthand must normalize to `InlinePolicy`; explicit definitions must supply either `PolicyReference` or `InlinePolicy`.

## Policy contract

```ts
type PolicyConstraint =
  | NumberBoundsConstraint
  | MaxDeltaConstraint
  | CooldownConstraint
  | EvidenceQualityConstraint
  | ModelUncertaintyConstraint
  | ExpectedOutcomeConstraint
  | SampleSizeConstraint
  | PauseConstraint;

type NumberBoundsConstraint = {
  kind: "number-bounds";
  min: number;
  max: number;
};

type MaxDeltaConstraint = {
  kind: "max-delta";
  value: number;
};

type CooldownConstraint = {
  kind: "cooldown";
  seconds: number;
};

type EvidenceQualityConstraint = {
  kind: "min-evidence-quality";
  value: number;
};

type ModelUncertaintyConstraint = {
  kind: "max-model-uncertainty";
  value: number;
};

type ExpectedOutcomeConstraint = {
  kind: "min-expected-outcome";
  value: number;
};

type SampleSizeConstraint = {
  kind: "min-sample-size";
  value: number;
};

type PauseConstraint = {
  kind: "pause";
  paused: boolean;
};

type PolicyEvaluationResult = {
  result: "approved" | "blocked" | "fallback";
  reasons: string[];
  appliedConstraints: string[];
};
```

Rules:

- Policy reason codes should be stable strings.
- Runtime should return fallback when policy result is `fallback`.
- Runtime should not return a candidate value as approved when policy result is `blocked`.

## Decision strategy

```ts
type DecisionStrategy =
  | FixedValueStrategy
  | NumericRuleStrategy;

type FixedValueStrategy = {
  kind: "fixed-value";
  id: string;
  value: DecisionValue;
};

type NumericRuleStrategy = {
  kind: "numeric-rule";
  id: string;
  baseValue: number;
  min: number;
  max: number;
  step: number;
  cooldownSeconds: number;
  rules: NumericAdjustmentRule[];
};

type NumericAdjustmentRule = {
  id: string;
  when: RuntimeCondition;
  adjustBy: number;
  reason: string;
};

type RuntimeCondition = {
  all?: RuntimeCondition[];
  any?: RuntimeCondition[];
  fact?: string;
  operator?: "eq" | "neq" | "gt" | "gte" | "lt" | "lte";
  value?: RuntimeContextValue;
};
```

Rules:

- Strategy execution must be bounded and deterministic in the online runtime path.
- Strategies must not produce values outside the contract action space.
- `NumericRuleStrategy` is the only adaptive strategy required for the MVP.
- Future strategy types should extend `DecisionStrategy` without changing `DecideResponse`.

## Decision state

```ts
type DecisionState = {
  definition: DecisionDefinitionRef;
  controlTarget?: DecisionTargetRef;
  runtimeTarget?: DecisionTargetRef;
  lifecycle: LifecycleState;
  activeValue?: DecisionValue;
  activeStrategy?: DecisionStrategy;
  previousValue?: DecisionValue;
  lastDecisionAt?: string;
  cooldownUntil?: string;
  paused?: boolean;
  override?: OperatorOverride;
};

type OperatorOverride = {
  value: DecisionValue;
  reason: string;
  setBy?: string;
  expiresAt?: string;
};
```

Rules:

- `DecisionState` owns live runtime authority; `DecisionDefinition` owns declared semantics.
- `activeStrategy` is how async intelligence or operator tooling affects online adaptation.
- Operator override takes precedence over active strategy unless policy says otherwise.
- Governed control state is keyed by decision definition plus control target.
- Runtime target state is keyed by decision definition plus runtime target.

## Evidence snapshot

```ts
type EvidenceSnapshot = {
  definition?: DecisionDefinitionRef;
  evidenceView: EvidenceViewRef;
  capturedAt: string;
  freshnessSeconds?: number;
  sampleSize?: number;
  confidence?: ConfidenceReport;
  metrics: Record<string, number>;
  quality: "missing" | "insufficient" | "sufficient" | "stale" | "conflicting";
};

type ConfidenceReport = {
  evidenceQuality?: number;
  modelUncertainty?: number;
  expectedOutcome?: number;
};

type EvidenceViewRef = {
  signal: SignalRef;
  target: DecisionTargetRef;
  window?: string;
  filters?: Record<string, RuntimeContextValue>;
};
```

Rules:

- The MVP can use in-memory or fixture evidence.
- Evidence details should be available to audit, but runtime responses should stay compact.
- Missing evidence should not crash runtime; it should flow into policy and fallback semantics.
- `window` is allowed for raw events and app-emitted metrics. It must be omitted for a fixed-window derived signal because that signal's immutable key already owns its aggregation window.

## Runtime API contracts

Code-first SDKs may combine definition authoring and runtime binding in one ergonomic object. Before hashing or transport, the SDK/tooling must partition that object into the immutable `DecisionDefinition` and the runtime `DecideRequest` below. Bound values must never affect definition identity.

```ts
type DecideRequest = {
  decisionKey: string;
  definition?: DecisionDefinitionRef;
  runtimeTarget?: DecisionTargetRef;
  runtimeContext: RuntimeContext;
  inputs?: SignalInput[];
  expectedContract: RuntimeContractIdentity;
  client: {
    appId: string;
    environment: string;
    sdk?: string;
    sdkVersion?: string;
  };
  correlationId?: string;
};

type SignalInput = {
  signal: SignalRef;
  value: RuntimeContextValue;
};

type TargetResolutionProvenance = {
  targetType: TargetType;
  claimedId?: string;
  resolvedId: string;
  source:
    | "client-claimed"
    | "client-verified"
    | "server-derived"
    | "server-replaced";
};

type ServerDecisionResult<T extends DecisionValue = DecisionValue> = {
  decisionKey: string;
  definition?: DecisionDefinitionRef;
  decisionId: string;
  value: T;
  valueType: ValueType;
  decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
  strategyId?: string;
  confidence: ConfidenceReport | null;
  reason: string;
  auditId: string;
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
  exposure?: {
    confirmationRequired: boolean;
    confirmToken?: string;
  };
  policy: PolicyEvaluationResult;
  definitionStatus: ContractRuntimeStatus;
};

type ClientFallbackResult<T extends DecisionValue = DecisionValue> = {
  decisionKey: string;
  value: T;
  valueType: ValueType;
  decisionMode: "fallback";
  confidence: null;
  reason: string;
  fallback: {
    source: "client-fallback";
    resolutionFallbackUsed: false;
    decisionFallbackUsed: true;
    reason: string;
  };
};

type DecisionResult<T extends DecisionValue = DecisionValue> =
  | ServerDecisionResult<T>
  | ClientFallbackResult<T>;

type DecideResponse<T extends DecisionValue = DecisionValue> =
  ServerDecisionResult<T>;

type RuntimeDecisionResult<T extends DecisionValue = DecisionValue> =
  ServerDecisionResult<T>;

type ExposureConfirmationRequest = {
  confirmToken: string;
  appliedAt?: string;
  correlationId?: string;
};

type ExposureConfirmationResult = {
  exposureId: string;
  decisionId: string;
  status: "confirmed";
  confirmedAt: string;
};
```

Rules:

- The response returns the final concrete value for application code.
- `decisionMode` explains how the value was produced without exposing internals.
- The server wire response always contains `decisionId`, `auditId`, `policy`, and `definitionStatus`.
- A local client fallback caused by data-plane unavailability is an SDK-produced `ClientFallbackResult`; it is disabled unless explicitly configured and cannot claim server policy, definition status, decision, audit, or exposure identity.
- A 4xx contract/configuration response can never produce `ClientFallbackResult`.
- `confidence` is `null` for static server decision fallback.
- Resolution fallback can still return a real confidence report if a broader target produced an approved decision.
- Client target/cohort claims are context, not authority. `targetProvenance` records whether each effective target was client-claimed, verified, server-derived, or replaced.
- Default runtime responses contain compact confidence and target provenance; full evidence-view references remain in audit records.
- `exposure.confirmToken` lets an SDK confirm exposure after the application applies or renders the returned value. The initial `RuntimeDecisionResult` must not include an `exposureId`; exposure identity is created by confirmation.
- `definitionStatus.integrity` indicates whether the client expectation matched a registered known contract definition.
- The full definition bundle is not sent with each request; only compact identity is sent.
- Exposure confirmation is idempotent for the same decision/token and creates the first `exposureId` under accepted decision A4.

## Canonical definition normalization and digest

Every authoring surface must normalize into the same language-neutral `DecisionDefinition` before compatibility comparison or hashing. Combined code-first and explicit forms that express the same semantics must produce byte-identical canonical definitions and therefore the same digest.

Normalization:

1. Remove all bound runtime values.
2. Convert each bound signal input into its immutable `SignalRef`.
3. Convert each typed target binding into a runtime-context schema entry. The original object property name is the canonical context field name; for example, `sessionId: flaggo.target.session(value)` becomes `sessionId: { type: "string", target: "session" }`.
4. Normalize SDK-specific policy shorthand, such as client-library [`PolicyAuthoring`](../client-library/README.md#policy-authoring-normalization), into canonical `InlinePolicy` constraints.
5. Materialize generated fields such as `signals.allowed` from role references.
6. Reduce every signal role/reference to immutable `SignalRef { key }`. `schemaDigest` belongs to signal-declaration conflict detection and is excluded from decision-definition identity.
7. Omit undefined fields and normalize equivalent optional/default forms according to the contract version.
8. Sort JSON object keys recursively.
9. Reject duplicate signal keys, duplicate context fields, duplicate policy constraint kinds, or conflicting role/schema declarations.
10. Serialize with RFC 8785 JSON Canonicalization Scheme.
11. Compute SHA-256 over the canonical UTF-8 bytes and encode the identity as `sha256:<lowercase-hex>`.

Order-sensitive arrays retain authored order because order changes behavior:

- `targetHierarchy`,
- `inference.fallbackOrder`,
- prioritized objective lists such as `intent.secondary`,
- rollout stages,
- tuple-like values such as numeric ranges.

Order-insensitive collections are duplicate-free sets and are sorted by immutable signal key:

- `signals.evidence`,
- `signals.guardrails`,
- generated `signals.allowed`,
- canonical `inference.inputs`,
- signal declarations in a bundle,
- `InlinePolicy.constraints`, sorted by constraint kind.

For `bundleDigest`, decision definitions are sorted by stable decision key after each definition has been normalized. Duplicate decision keys with different definition digests are a `contract-conflict`; identical duplicates are deduplicated.

Runtime wire `inputs` are also key-sorted for deterministic transport and audit comparison. Duplicate signal keys are invalid; clients and servers must reject them rather than applying first-wins or last-wins behavior.

## Contract identity and integrity

```ts
type ContractIdentity = {
  definitionId?: string;
  bundleDigest?: string;
  contractDigest?: string;
  revision?: string;
  buildId?: string;
  deploymentId?: string;
  artifactDigest?: string;
};

type RuntimeContractIdentity = ContractIdentity & {
  definitionId: string;
  contractDigest: string;
  revision: string;
};

type ContractIntegrityState =
  | "verified"
  | "known-older-revision"
  | "unknown-client-contract"
  | "contract-conflict"
  | "unknown-decision-key"
  | "retired-decision-key";

type ContractRuntimeStatus = {
  revision?: string;
  bundleDigest?: string;
  contractDigest?: string;
  buildId?: string;
  deploymentId?: string;
  integrity: ContractIntegrityState;
  compatibility?: ContractCompatibility;
};

type ContractCompatibility =
  | "identical"
  | "metadata-only"
  | "new-contract-required";
```

Rules:

- `verified` and `known-older-revision` may return approved decisions when the registry recognizes the caller's immutable definition revision.
- Missing identity, `unknown-client-contract`, `contract-conflict`, `unknown-decision-key`, and `retired-decision-key` are contract/configuration errors. They return Problem Details and cannot become server or SDK-local fallback.
- Browser-provided definition identity is useful for drift detection, not as a security boundary.
- Multiple builds of the same service may be deployed at the same time. Runtime integrity must be evaluated against the expected definition identity carried by the calling build, not a singular environment-wide bundle.
- `definitionId` identifies the immutable semantic decision definition used by the caller. `contractDigest` identifies canonical definition content. `bundleDigest` identifies the full submitted bundle. `buildId`, `deploymentId`, and `artifactDigest` identify the workload instance or release that carries the contract expectation.
- Semantic changes should not overwrite an existing `definitionId`. They should be rejected under the old ID and registered as a new definition ID or explicit semantic revision.
- Known older revisions are served only when the request identifies that exact registered revision and lifecycle permits it. The runtime never substitutes another revision.

## Contract, telemetry, evidence, and state reuse

Flaggo should avoid sharing unsafe learned decision behavior across different contracts, but it should not throw away useful historical observations.

| Layer | Default sharing behavior | Reason |
| --- | --- | --- |
| Raw telemetry observations | Share across definitions with the same application, signal key, and target semantics. | Observations are historical facts, not learned policy. |
| Evidence views | Share only when signal key, target, window, and filters match. | A metric can be reused if it is the same immutable signal viewed the same way. |
| Decision state or active strategy | Isolate by definition ID and control/runtime target. | A learned value or strategy for one definition may be unsafe for another. |

Rules:

- Telemetry identity should be stable at the event/signal level so a new definition can reuse existing observations for unchanged inputs.
- Evidence views should be identified by immutable signal key plus target, window, and filters.
- A new definition may start in partial-warm mode: reused evidence can contribute immediately, while new signals collect data until policy marks them sufficient.
- Decision state, active strategies, cooldowns, and operator overrides are keyed by definition ID plus resolved control/runtime target. They are not inherited automatically across definitions.
- State migration between definition IDs should be an explicit operator or registry action, not an implicit compatibility rule.

## Proposal contracts

```ts
type DecisionProposal =
  | ValueProposal
  | StrategyProposal
  | ExperimentProposal
  | HoldProposal
  | RollbackProposal;

type ValueProposal = {
  proposalType: "value";
  decisionKey: string;
  target: DecisionTargetRef;
  value: DecisionValue;
  confidence: ConfidenceReport | null;
  evidenceStatus: string;
  rationale: string;
  risks: string[];
};

type StrategyProposal = {
  proposalType: "strategy";
  decisionKey: string;
  target: DecisionTargetRef;
  strategy: DecisionStrategy;
  confidence: ConfidenceReport | null;
  evidenceStatus: string;
  rationale: string;
  risks: string[];
};

type ExperimentProposal = {
  proposalType: "experiment";
  decisionKey: string;
  target: DecisionTargetRef;
  candidates: DecisionValue[];
  rationale: string;
  risks: string[];
};

type HoldProposal = {
  proposalType: "hold";
  decisionKey: string;
  target: DecisionTargetRef;
  reason: string;
};

type RollbackProposal = {
  proposalType: "rollback";
  decisionKey: string;
  target: DecisionTargetRef;
  reason: string;
};

type GovernanceOutcome = {
  result: "approved" | "limited" | "experiment" | "hold" | "rollback" | "fallback" | "requires-approval" | "rejected";
  reasons: string[];
  activatedState?: DecisionState;
};
```

Rules:

- MVP async intelligence may be scripted, but it must produce `DecisionProposal`.
- Governance activation writes `DecisionState`.
- Online runtime consumes `DecisionState`, not raw proposal text.

## Audit record

```ts
type AuditDecisionResult<T extends DecisionValue = DecisionValue> =
  Omit<ServerDecisionResult<T>, "exposure"> & {
    exposure?: {
      confirmationRequired: boolean;
    };
  };

type AuditRecord = {
  auditId?: string;
  timestamp: string;
  decisionKey: string;
  request?: DecideRequest;
  response?: AuditDecisionResult;
  contractVersion?: string;
  runtimeTarget?: DecisionTargetRef;
  controlTarget?: DecisionTargetRef;
  evidence?: EvidenceSnapshot;
  stateSummary?: {
    decisionMode: DecideResponse["decisionMode"];
    strategyId?: string;
    paused?: boolean;
    overrideUsed?: boolean;
  };
  policy?: PolicyEvaluationResult;
  proposal?: DecisionProposal;
  governanceOutcome?: GovernanceOutcome;
  reason: string;
};
```

Rules:

- Audit records may contain more detail than runtime responses.
- Confirmation tokens are capabilities and must be removed before constructing `AuditRecord`; audit response projections can retain `confirmationRequired` but never `confirmToken`.
- Audit should be local-first in MVP, such as console, file, or SQLite.
- Cloud audit sinks should implement `IAuditSink`; they should not change the audit contract.

## Contract bundle and registration receipt

```ts
type DecisionDefinitionBundle = {
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
  definitions: DecisionDefinitionBundleEntry[];
};

type DecisionDefinitionBundleEntry = {
  definitionId: string;
  key: string;
  owner?: string;
  valueType: ValueType;
  actionSpace: ActionSpace;
  runtimeContextSchema?: RuntimeContextSchema;
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: InferenceDeclaration;
  intent?: DecisionIntent;
  fallback: FallbackContract;
  policy: PolicyReference | InlinePolicy;
  onlineStrategy?: OnlineStrategyDeclaration;
};

type DefinitionBundleValidationResult = {
  status: "valid" | "invalid";
  bundleDigest?: string;
  contractDigest?: string;
  compatibility?: ContractCompatibility;
  issues: ContractIssue[];
};

type ContractIssue = {
  code: string;
  severity: "error" | "warning";
  path: string;
  message: string;
  decisionKey?: string;
  signalKey?: string;
};

type ContractChange = {
  kind: "created" | "metadata-updated" | "deprecation-candidate" | "semantic-conflict";
  decisionKey: string;
  fromRevision?: string;
  toRevision?: string;
};

type RegistrationReceipt = {
  application: string;
  environment: string;
  bundleDigest: string;
  contractDigest: string;
  buildId?: string;
  artifactDigest?: string;
  registeredRevisions: Record<string, string>;
  compatibility: ContractCompatibility;
  status: "approved";
  changes?: ContractChange[];
  issues: ContractIssue[];
};

type DefinitionBundleApplyResult =
  | RegistrationReceipt
  | {
      status: "requires-approval";
      approvalRequestId: string;
      application: string;
      environment: string;
      bundleDigest: string;
      contractDigest: string;
      compatibility: "new-contract-required";
      changes: ContractChange[];
      issues: ContractIssue[];
    }
  | {
      status: "rejected";
      issues: ContractIssue[];
    };

type DefinitionBundleApprovalResult =
  | {
      approvalRequestId: string;
      status: "pending" | "rejected";
    }
  | {
      approvalRequestId: string;
      status: "approved";
      receipt: RegistrationReceipt;
    };

type ResourceOwnershipManifest = DecisionDefinitionBundle;
type ManifestValidationResult = DefinitionBundleValidationResult;
```

Rules:

- `DecisionDefinitionBundle` is the canonical language-neutral sync artifact.
- SDK-generated declarations, hand-authored JSON/YAML, GitOps workflows, and registry exports should all produce or reference the same bundle shape.
- Bundle sync creates or validates decision definition revisions.
- Definition IDs are semantic ownership boundaries. A semantic conflict under an existing definition ID is rejected; the caller must create or allow tooling to create a new definition ID/revision.
- Missing bundle resources become deprecation candidates, not deletes.
- Bundle data must remain provider-neutral.
- `ResourceOwnershipManifest` is retained only as a compatibility alias while the design migrates to `DecisionDefinitionBundle`.
- Build metadata is allowed in the bundle for traceability, but compatibility should be based on canonical definition content, not incidental build metadata. Two different builds with identical decision definitions may share the same `contractDigest` while having different `buildId` or `artifactDigest`.
- Registration receipts should identify revisions per decision key because one bundle can contain multiple decision definitions.
- Registry-managed versioning should be the default UX. Developers should not need to hand-name every version; tooling can keep a stable definition ID for metadata-only changes and mint a new semantic revision or ID when the definition changes incompatibly.
- Bundle validation issues use stable machine-readable codes and JSON Pointer paths; clients must not parse prose messages.
- Bundle apply is atomic under accepted decision A5. Under accepted decision A6, semantic changes return `requires-approval` without mutation; explicit approval atomically applies the pending canonical bundle and produces the receipt.

## Tetris MVP contract example

Signal declarations are extracted from producer-owned typed handles:

```json
[
  { "kind": "metric", "key": "tetris.boardPressure", "type": "number", "source": "app-emitted", "range": [0, 1] },
  { "kind": "metric", "key": "tetris.recentPlacementTimeMs", "type": "number", "source": "app-emitted", "unit": "ms" },
  { "kind": "metric", "key": "tetris.recoveryFailures", "type": "number", "source": "app-emitted" },
  { "kind": "metric", "key": "tetris.currentLevel", "type": "number", "source": "app-emitted" },
  {
    "kind": "event",
    "key": "tetris.piecePlaced",
    "fields": { "placementTimeMs": "number", "hardDrop": "boolean" },
    "units": { "placementTimeMs": "ms" }
  },
  {
    "kind": "event",
    "key": "tetris.sessionEnded",
    "fields": { "endReason": "string", "durationSeconds": "number" },
    "units": { "durationSeconds": "s" }
  },
  {
    "kind": "metric",
    "key": "tetris.earlyLossRate24h",
    "type": "number",
    "source": "derived",
    "from": [{ "key": "tetris.sessionEnded" }],
    "aggregation": "rate(endReason == 'early_loss')",
    "window": "24h"
  },
  {
    "kind": "metric",
    "key": "tetris.hardDropRate24h",
    "type": "number",
    "source": "derived",
    "from": [{ "key": "tetris.piecePlaced" }],
    "aggregation": "rate(hardDrop == true)",
    "window": "24h"
  }
]
```

The decision definition references those signal identities without redefining their schemas:

`signals.allowed` below is generated from role references such as `intent`, `inference.inputs`, evidence, and guardrails. Authors should not maintain a duplicate flat allowlist by hand.

```json
{
  "ref": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "revision": "1"
  },
  "lifecycle": "active",
  "valueType": "number",
  "actionSpace": {
    "type": "number",
    "min": 200,
    "max": 1500,
    "step": 50,
    "default": 800
  },
  "fallback": {
    "value": 800,
    "reason": "safe_default_drop_interval"
  },
  "runtimeContextSchema": {
    "userId": { "type": "string", "target": "user" },
    "sessionId": { "type": "string", "target": "session" },
    "cohort": { "type": "string", "target": "cohort" },
    "deviceType": { "type": "string" }
  },
  "targetHierarchy": ["session", "user", "cohort", "global"],
  "signals": {
    "allowed": [
      { "key": "tetris.boardPressure" },
      { "key": "tetris.currentLevel" },
      { "key": "tetris.earlyLossRate24h" },
      { "key": "tetris.hardDropRate24h" },
      { "key": "tetris.piecePlaced" },
      { "key": "tetris.recentPlacementTimeMs" },
      { "key": "tetris.recoveryFailures" },
      { "key": "tetris.sessionEnded" }
    ]
  },
  "inference": {
    "target": "session",
    "inputs": [
      { "key": "tetris.boardPressure" },
      { "key": "tetris.currentLevel" },
      { "key": "tetris.recentPlacementTimeMs" },
      { "key": "tetris.recoveryFailures" }
    ],
    "fallbackOrder": ["cohort", "global"]
  },
  "intent": {
    "type": "metric-objective",
    "primary": { "signal": { "key": "tetris.earlyLossRate24h" }, "direction": "minimize" },
    "secondary": [
      { "signal": { "key": "tetris.hardDropRate24h" }, "direction": "target", "target": 0.45 },
      { "signal": { "key": "tetris.recentPlacementTimeMs" }, "direction": "minimize" }
    ],
    "rationale": "Keep gameplay challenging but playable while reducing early frustration."
  },
  "onlineStrategy": {
    "mode": "approved-strategy",
    "liveInputs": [
      "currentLevel",
      "boardPressure",
      "recentPlacementTimeMs",
      "recoveryFailures"
    ]
  },
  "policy": {
    "kind": "inline",
    "constraints": [
      { "kind": "cooldown", "seconds": 20 },
      { "kind": "max-delta", "value": 50 },
      { "kind": "max-model-uncertainty", "value": 0.35 },
      { "kind": "min-evidence-quality", "value": 0.7 },
      { "kind": "min-sample-size", "value": 30 },
      { "kind": "number-bounds", "min": 200, "max": 1500 }
    ]
  }
}
```

## MVP domain freeze and provisional wire contracts

For the first implementation, treat these provider-neutral domain contracts as frozen:

- result value primitives,
- action space shapes,
- `DecisionTargetRef`,
- `DecisionDefinition`,
- `DecisionStrategy` with `fixed-value` and `numeric-rule`,
- `DecisionState`,
- `PolicyEvaluationResult`,
- `AuditRecord`,
- `DecisionDefinitionBundle`,
- `ContractIdentity`,
- `ContractRuntimeStatus`,
- `ContractCompatibility`.

The HTTP projections of `DecideRequest`, `ServerDecisionResult`, exposure confirmation, validation/apply/approval results, and `RegistrationReceipt` remain provisional until the accepted [Phase 1 API Contract Proposal](../API_CONTRACT_PROPOSAL.md) decisions are encoded in OpenAPI, JSON Schema, fixtures, and passing conformance tests. After that freeze, future wire changes should be additive unless an implementation proves the contract unusable.
