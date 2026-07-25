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
2. Decision surfaces must be pre-registered or validated by manifest before production runtime.
3. Adaptive behavior is represented by a `DecisionStrategy`, not hidden application logic.
4. Online runtime returns a concrete value, even when that value came from a strategy.
5. Async intelligence produces proposals; governance activates values, strategies, experiments, holds, rollbacks, or fallbacks.
6. Runtime responses must include scope, fallback, policy, and audit metadata.
7. New strategy types, storage backends, evidence sources, and policy rules must extend explicit interfaces instead of changing the runtime response shape.

## Primitive types

```ts
type ValueType = "boolean" | "number" | "string";
type DecisionValue = boolean | number | string;
type LifecycleState = "active" | "deprecated" | "retired";

type BuiltInScopeType = "session" | "user" | "segment" | "global";
type ScopeType = BuiltInScopeType | (string & {});

type ScopeRef = {
  type: ScopeType;
  id: string;
};

type DecisionSurfaceRef = {
  appId: string;
  environment: string;
  surface: string;
  contractVersion?: string;
};

type RuntimeContextValue = boolean | number | string | null;
type RuntimeContext = Record<string, RuntimeContextValue>;
```

Rules:

- `surface` uses stable domain naming such as `tetris.dropInterval`.
- `environment` is provider-neutral, such as `dev`, `test`, `prod`.
- `ScopeRef.type` is extensible, but MVP built-ins are `session`, `user`, `segment`, and `global`.
- Runtime context must stay primitive and JSON-serializable for audit and policy evaluation.

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
type DecisionContract = {
  ref: DecisionSurfaceRef;
  lifecycle: LifecycleState;
  valueType: ValueType;
  actionSpace: ActionSpace;
  scopeHierarchy: string[];
  fallback: FallbackContract;
  runtimeContextSchema?: RuntimeContextSchema;
  evidenceRequirements?: EvidenceRequirement[];
  goals?: GoalDefinition[];
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
  }
>;

type EvidenceRequirement = {
  name: string;
  source: string;
  window?: string;
  scope?: string;
};

type GoalDefinition = {
  name: string;
  direction: "minimize" | "maximize" | "target";
  target?: number;
};

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
  | ConfidenceConstraint
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

type ConfidenceConstraint = {
  kind: "min-confidence";
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
  surface: string;
  scope: ScopeRef;
  contractVersion: string;
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

- `DecisionState` owns live runtime authority; `DecisionContract` owns declared semantics.
- `activeStrategy` is how async intelligence or operator tooling affects online adaptation.
- Operator override takes precedence over active strategy unless policy says otherwise.

## Evidence snapshot

```ts
type EvidenceSnapshot = {
  surface: string;
  scope: ScopeRef;
  capturedAt: string;
  freshnessSeconds?: number;
  sampleSize?: number;
  confidence?: number;
  metrics: Record<string, number>;
  quality: "missing" | "insufficient" | "sufficient" | "stale" | "conflicting";
};
```

Rules:

- The MVP can use in-memory or fixture evidence.
- Evidence details should be available to audit, but runtime responses should stay compact.
- Missing evidence should not crash runtime; it should flow into policy and fallback semantics.

## Runtime API contracts

```ts
type DecideRequest = {
  surface: string;
  requestedScope?: ScopeRef;
  runtimeContext: RuntimeContext;
  expectedContract?: ContractIdentity;
  client: {
    appId: string;
    environment: string;
    sdk?: string;
    sdkVersion?: string;
  };
  correlationId?: string;
};

type DecideResponse<T extends DecisionValue = DecisionValue> = {
  surface: string;
  value: T;
  valueType: ValueType;
  decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
  strategyId?: string;
  confidence: number | null;
  reason: string;
  auditId: string;
  requestedScope?: ScopeRef;
  resolvedScope: ScopeRef;
  evidenceScope?: ScopeRef;
  resolutionChain: string[];
  fallback: {
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: boolean;
    reason: string | null;
  };
  policy: PolicyEvaluationResult;
  contract: ContractRuntimeStatus;
};
```

Rules:

- The response returns the final concrete value for application code.
- `decisionMode` explains how the value was produced without exposing internals.
- `confidence` is `null` for static decision fallback.
- Resolution fallback can still return a real confidence score if a broader scope produced an approved decision.
- `contract.integrity` indicates whether the client expectation matched a registered compatible contract.
- The full contract bundle is not sent with each request; only compact identity is sent.

## Contract identity and integrity

```ts
type ContractIdentity = {
  digest?: string;
  revision?: string;
  deploymentId?: string;
};

type ContractIntegrityState =
  | "verified"
  | "compatible-drift"
  | "unknown-client-contract"
  | "incompatible-drift"
  | "unknown-surface"
  | "retired-surface";

type ContractRuntimeStatus = {
  revision?: string;
  digest?: string;
  integrity: ContractIntegrityState;
  compatibility?: ContractCompatibility;
};

type ContractCompatibility =
  | "identical"
  | "backward-compatible"
  | "requires-client-update"
  | "unsafe";
```

Rules:

- `verified` and `compatible-drift` may return approved decisions.
- `unknown-client-contract`, `incompatible-drift`, `unknown-surface`, and `retired-surface` should fall back in production enforcement mode.
- Browser-provided contract identity is useful for drift detection, not as a security boundary.

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
  surface: string;
  targetScope: ScopeRef;
  value: DecisionValue;
  confidence: number | null;
  evidenceStatus: string;
  rationale: string;
  risks: string[];
};

type StrategyProposal = {
  proposalType: "strategy";
  surface: string;
  targetScope: ScopeRef;
  strategy: DecisionStrategy;
  confidence: number | null;
  evidenceStatus: string;
  rationale: string;
  risks: string[];
};

type ExperimentProposal = {
  proposalType: "experiment";
  surface: string;
  targetScope: ScopeRef;
  candidates: DecisionValue[];
  rationale: string;
  risks: string[];
};

type HoldProposal = {
  proposalType: "hold";
  surface: string;
  targetScope: ScopeRef;
  reason: string;
};

type RollbackProposal = {
  proposalType: "rollback";
  surface: string;
  targetScope: ScopeRef;
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
  surface: string;
  request?: DecideRequest;
  response?: DecideResponse;
  contractVersion?: string;
  resolvedScope?: ScopeRef;
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
type ContractBundle = {
  format: "flaggo.contract-bundle/v1";
  application: {
    id: string;
    environment: string;
  };
  source: {
    repository?: string;
    path?: string;
    commit?: string;
  };
  surfaces: ContractBundleSurface[];
};

type ContractBundleSurface = {
  name: string;
  owner?: string;
  valueType: ValueType;
  actionSpace: ActionSpace;
  scopeHierarchy: string[];
  runtimeContextSchema?: RuntimeContextSchema;
  fallback: FallbackContract;
  onlineStrategy?: OnlineStrategyDeclaration;
};

type ContractBundleValidationResult = {
  result: "valid" | "invalid";
  digest?: string;
  errors: string[];
  warnings: string[];
};

type RegistrationReceipt = {
  application: string;
  environment: string;
  bundleDigest: string;
  registeredRevision: string;
  compatibility: ContractCompatibility;
  status: "approved" | "rejected" | "requires-approval";
};

type ResourceOwnershipManifest = ContractBundle;
type ManifestValidationResult = ContractBundleValidationResult;
```

Rules:

- `ContractBundle` is the canonical language-neutral sync artifact.
- SDK-generated declarations, hand-authored JSON/YAML, GitOps workflows, and registry exports should all produce or reference the same bundle shape.
- Bundle sync creates or validates contract revisions.
- Missing bundle resources become deprecation candidates, not deletes.
- Bundle data must remain provider-neutral.
- `ResourceOwnershipManifest` is retained only as a compatibility alias while the design migrates to `ContractBundle`.

## Tetris MVP contract example

```json
{
  "ref": {
    "appId": "tetris-demo",
    "environment": "dev",
    "surface": "tetris.dropInterval",
    "contractVersion": "1"
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
  "scopeHierarchy": ["session", "user", "segment", "global"],
  "fallback": {
    "value": 800,
    "reason": "safe_default_drop_interval"
  },
  "runtimeContextSchema": {
    "userId": { "type": "string", "required": true },
    "sessionId": { "type": "string", "required": true },
    "currentLevel": { "type": "number" },
    "boardPressure": { "type": "string" },
    "recentPlacementTimeMs": { "type": "number" },
    "recoveryFailures": { "type": "number" }
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
      { "kind": "min-confidence", "value": 0.7 }
    ]
  }
}
```

## MVP freeze

For the first implementation, treat these as frozen:

- result value primitives,
- action space shapes,
- `ScopeRef`,
- `DecisionContract`,
- `DecisionStrategy` with `fixed-value` and `numeric-rule`,
- `DecisionState`,
- `DecideRequest`,
- `DecideResponse`,
- `PolicyEvaluationResult`,
- `AuditRecord`,
- `ContractBundle`,
- `RegistrationReceipt`,
- `ContractIdentity`,
- `ContractRuntimeStatus`,
- `ContractCompatibility`.

Future changes should be additive unless an MVP implementation proves a contract is unusable.
