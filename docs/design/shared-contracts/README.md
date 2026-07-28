# Shared Contracts Design

## Purpose

This document freezes the shared MVP contract shapes used across the Flaggo TypeScript SDK, Decision API, local adapters, tests, manifests, and future cloud adapters.

The goal is not to finalize every future field. The goal is to define a small, stable set of provider-neutral interfaces that can serve the Tetris MVP while leaving clear extension seams.

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
  schemaDigest?: string;
};

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

type MetricObjective = {
  signal: SignalRef;
  direction: "minimize" | "maximize" | "target";
  target?: number;
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
type DecisionDefinitionRef = {
  appId: string;
  environment: string;
  key: string;
  revision: string;
};

type DecisionTargetRef = {
  type: string;
  id: string;
};

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
  expectedContract?: ContractIdentity;
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

type DecideResponse<T extends DecisionValue = DecisionValue> = {
  decisionKey: string;
  definition?: DecisionDefinitionRef;
  decisionId?: string;
  value: T;
  valueType: ValueType;
  decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
  strategyId?: string;
  confidence: ConfidenceReport | null;
  reason: string;
  auditId: string;
  runtimeTarget?: DecisionTargetRef;
  controlTarget?: DecisionTargetRef;
  evidenceViews?: EvidenceViewRef[];
  resolutionChain: string[];
  fallback: {
    source: "server" | "client-fallback";
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: boolean;
    reason: string | null;
  };
  exposure?: {
    confirmationRequired: boolean;
    confirmToken?: string;
  };
  policy?: PolicyEvaluationResult;
  definitionStatus?: ContractRuntimeStatus;
};
```

Rules:

- The response returns the final concrete value for application code.
- `decisionMode` explains how the value was produced without exposing internals.
- `decisionId`, `auditId`, `policy`, and `definitionStatus` are present only for server-produced results. A local client fallback caused by service unavailability uses `fallback.source = "client-fallback"` and cannot claim server policy, decision, or audit identity.
- `confidence` is `null` for static server decision fallback.
- Resolution fallback can still return a real confidence report if a broader target produced an approved decision.
- `exposure.confirmToken` lets an SDK confirm exposure after the application applies or renders the returned value. The initial `RuntimeDecisionResult` must not include an `exposureId`; exposure identity is created by confirmation.
- `definitionStatus.integrity` indicates whether the client expectation matched a registered known contract definition.
- The full definition bundle is not sent with each request; only compact identity is sent.

## Canonical definition normalization and digest

Every authoring surface must normalize into the same language-neutral `DecisionDefinition` before compatibility comparison or hashing. Combined code-first and explicit forms that express the same semantics must produce byte-identical canonical definitions and therefore the same digest.

Normalization:

1. Remove all bound runtime values.
2. Convert each bound signal input into its immutable `SignalRef`.
3. Convert each typed target binding into a runtime-context schema entry. The original object property name is the canonical context field name; for example, `sessionId: flaggo.target.session(value)` becomes `sessionId: { type: "string", target: "session" }`.
4. Materialize generated fields such as `signals.allowed` from role references.
5. Omit undefined fields and normalize equivalent optional/default forms according to the contract version.
6. Sort JSON object keys recursively.
7. Reject duplicate signal keys, duplicate context fields, or conflicting role/schema declarations.
8. Serialize with RFC 8785 JSON Canonicalization Scheme.
9. Compute SHA-256 over the canonical UTF-8 bytes and encode the identity as `sha256:<lowercase-hex>`.

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
- signal declarations in a bundle.

For `bundleDigest`, decision definitions are sorted by stable decision key after each definition has been normalized. Duplicate decision keys with different definition digests are a `contract-conflict`; identical duplicates are deduplicated.

Runtime wire `inputs` are also key-sorted for deterministic transport and audit comparison. Duplicate signal keys are invalid; clients and servers must reject them rather than applying first-wins or last-wins behavior.

## Contract identity and integrity

```ts
type ContractIdentity = {
  contractId?: string;
  bundleDigest?: string;
  contractDigest?: string;
  revision?: string;
  buildId?: string;
  deploymentId?: string;
  artifactDigest?: string;
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
- `unknown-client-contract`, `contract-conflict`, `unknown-decision-key`, and `retired-decision-key` should fall back in production enforcement mode.
- Browser-provided definition identity is useful for drift detection, not as a security boundary.
- Multiple builds of the same service may be deployed at the same time. Runtime integrity must be evaluated against the expected definition identity carried by the calling build, not a singular environment-wide bundle.
- `contractId` identifies the immutable semantic contract used by the caller. `contractDigest` identifies canonical contract content. `bundleDigest` identifies the full submitted bundle. `buildId`, `deploymentId`, and `artifactDigest` identify the workload instance or release that carries the contract expectation.
- Semantic contract changes should not overwrite an existing `contractId`. They should be rejected under the old ID and registered as a new contract ID or new explicit semantic version.

## Contract, telemetry, evidence, and state reuse

Flaggo should avoid sharing unsafe learned decision behavior across different contracts, but it should not throw away useful historical observations.

| Layer | Default sharing behavior | Reason |
| --- | --- | --- |
| Raw telemetry observations | Share across definitions with the same application, signal key, and target semantics. | Observations are historical facts, not learned policy. |
| Evidence views | Share only when signal key, target, window, and filters match. | A metric can be reused if it is the same immutable signal viewed the same way. |
| Decision state or active strategy | Isolate by contract ID and control/runtime target. | A learned value or strategy for one contract may be unsafe for another. |

Rules:

- Telemetry identity should be stable at the event/signal level so a new definition can reuse existing observations for unchanged inputs.
- Evidence views should be identified by immutable signal key plus target, window, and filters.
- A new definition may start in partial-warm mode: reused evidence can contribute immediately, while new signals collect data until policy marks them sufficient.
- Decision state, active strategies, cooldowns, and operator overrides are keyed by contract ID plus resolved control/runtime target. They are not inherited automatically across contracts.
- State migration between contract IDs should be an explicit operator or registry action, not an implicit compatibility rule.

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
  result: "approved" | "limited" | "experiment" | "hold" | "rollback" | "fallback" | "requires_approval" | "rejected";
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
type AuditRecord = {
  auditId?: string;
  timestamp: string;
  decisionKey: string;
  request?: DecideRequest;
  response?: DecideResponse;
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
  onlineStrategy?: OnlineStrategyDeclaration;
};

type DefinitionBundleValidationResult = {
  result: "valid" | "invalid";
  bundleDigest?: string;
  contractDigest?: string;
  errors: string[];
  warnings: string[];
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
  status: "approved" | "rejected" | "requires-approval";
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
    "userId": { "type": "string", "required": true },
    "sessionId": { "type": "string", "required": true },
    "deviceType": { "type": "string" }
  },
  "targetHierarchy": ["session", "user", "cohort", "global"],
  "signals": {
    "allowed": [
      { "key": "tetris.boardPressure" },
      { "key": "tetris.recentPlacementTimeMs" },
      { "key": "tetris.recoveryFailures" },
      { "key": "tetris.currentLevel" },
      { "key": "tetris.piecePlaced" },
      { "key": "tetris.sessionEnded" },
      { "key": "tetris.earlyLossRate24h" },
      { "key": "tetris.hardDropRate24h" }
    ]
  },
  "inference": {
    "target": "session",
    "inputs": [
      { "key": "tetris.boardPressure" },
      { "key": "tetris.recentPlacementTimeMs" },
      { "key": "tetris.recoveryFailures" },
      { "key": "tetris.currentLevel" }
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
      { "kind": "number-bounds", "min": 200, "max": 1500 },
      { "kind": "max-delta", "value": 50 },
      { "kind": "cooldown", "seconds": 20 },
      { "kind": "min-evidence-quality", "value": 0.7 },
      { "kind": "max-model-uncertainty", "value": 0.35 }
    ]
  }
}
```

## MVP freeze

For the first implementation, treat these as frozen:

- result value primitives,
- action space shapes,
- `DecisionTargetRef`,
- `DecisionDefinition`,
- `DecisionStrategy` with `fixed-value` and `numeric-rule`,
- `DecisionState`,
- `DecideRequest`,
- `DecideResponse`,
- `PolicyEvaluationResult`,
- `AuditRecord`,
- `DecisionDefinitionBundle`,
- `RegistrationReceipt`,
- `ContractIdentity`,
- `ContractRuntimeStatus`,
- `ContractCompatibility`.

Future changes should be additive unless an MVP implementation proves a contract is unusable.
