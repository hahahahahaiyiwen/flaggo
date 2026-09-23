# Shared Contracts Design

## Purpose

This document defines the shared MVP domain, data, and wire-contract shapes
used across the Flaggo TypeScript SDK, Decision API, local adapters, tests,
manifests, and future cloud adapters. Module service and infrastructure ports
remain beside the behavior that consumes them. The approved Phase 3
re-baseline replaces the original bundle v1 authority declaration rather than
adding a compatibility layer.

The goal is not to finalize every future field. The goal is to define a small,
stable set of provider-neutral cross-component shapes that can serve the
Tetris MVP while leaving clear extension seams.

The mapping of these domain contracts to HTTP is the accepted baseline in the
[Phase 1 API Contract Proposal](../API_CONTRACT_PROPOSAL.md) and its executable
artifacts under `contracts/`.

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
5. Authenticated bundle approval may activate a declared initial authority;
   later async intelligence or another authorized producer creates independent
   proposals for governance.
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
- `schemaDigest` is `sha256:<lowercase-hex>` over the RFC 8785 canonical signal declaration with `schemaDigest` omitted. Tooling recomputes and verifies it when supplied; a mismatch is invalid. Reusing one signal key with a different computed digest is a contract conflict. Signal references contain only `key`, so digest metadata cannot change decision-definition identity.
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
- `valueType`, action-space type, default, and fallback value form one discriminated contract and must use the same primitive type.
- Number `min` must not exceed `max`; default and fallback values must remain inside that inclusive range.
- Number `step`, when present, must be positive, and the default and fallback must be reachable from `min` by an integral number of steps.
- Number values must be within `min` and `max`.
- Number values must align to `step` when `step` is present.
- String values should use a non-empty, duplicate-free `allowedValues` set when the application expects known values; default and fallback values must belong to that set.

## Decision contract

```ts
type DecisionDefinition = {
  ref: DecisionDefinitionRef;
  status: LifecycleState;
  valueType: ValueType;
  actionSpace: ActionSpace;
  fallback: FallbackContract;
  runtimeContextSchema?: RuntimeContextSchema;
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: InferenceDeclaration;
  intent?: DecisionIntent;
  lifecycle: AuthorityLifecycleDeclaration;
  policy: PolicyReference | InlinePolicy;
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

// Serialized as SignalRef; registry validation additionally requires source = "app-emitted".
type AppEmittedNumericMetricRef = NumericMetricRef;

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

type PolicyReference = {
  kind: "reference";
  policyId: string;
};

type InlinePolicy = {
  kind: "inline";
  constraints: PolicyConstraint[];
};

type AuthorityLifecycleDeclaration =
  | {
      authorityMode: "bundle-approved";
      initialAuthority: InitialAuthority;
    }
  | {
      authorityMode: "proposal-managed";
    };

type InitialAuthority =
  | ActiveValueInitialAuthority
  | NumericRuleInitialAuthority;

type ActiveValueInitialAuthority = {
  controlTarget: DecisionTargetRef;
  kind: "active-value";
  value: DecisionValue;
  rationale: string;
};

type NumericRuleInitialAuthority = {
  controlTarget: DecisionTargetRef;
  kind: "numeric-rule";
  rule: NumericRuleDeclaration;
  rationale: string;
};

type NumericRuleDeclaration = {
  threshold: number;
  valueAtOrAbove: number;
  valueBelow: number;
  weightedInputs: NumericRuleInput[];
};
```

MVP rule: `tetris.dropInterval` uses
`lifecycle.authorityMode = "bundle-approved"` with a numeric rule whose signal
references are a subset of `inference.inputs`. The bundle supplies no trusted
proposal, activation, strategy, state, or approval identities.

Both current Phase 3 authority kinds are valid initial candidates.
`active-value` carries one value that must satisfy the definition action space
and applicable runtime policy. `numeric-rule` carries the deterministic rule
validated below.

`NumericRuleDeclaration.weightedInputs` must be non-empty. Each input must
reference one declared inference input that resolves to an app-emitted numeric
metric, use finite `minimum < maximum`, and have a finite nonnegative weight;
the finite total weight must be positive, and `threshold` must be finite in
`[0, 1]`. The executor computes:

```text
normalizedInput = clamp((value - minimum) / (maximum - minimum), 0, 1)
score = sum(normalizedInput * weight) / sum(weight)
```

Division by total weight is required even when authored weights do not sum to
`1`. `score >= threshold` selects `valueAtOrAbove`; otherwise it selects
`valueBelow`. Both branch values must satisfy the numeric action space and
applicable runtime policy. SDK authoring uses a branded numeric metric handle;
registry and activation validation enforce the same numeric-source rule.

Missing evidence explicitly required by runtime policy is a server evaluation
outcome, not a numeric-rule executor input or data-plane availability failure.
When governed fallback is permitted, the server returns the registered
fallback as an audited decision. Otherwise it returns fallback-ineligible
`required-evidence-unavailable` Problem Details. Definition policy never
authorizes an SDK-local value for this outcome.

`InferenceDeclaration.inputs` is structurally serialized as `SignalRef[]`, but every referenced key must resolve to an app-emitted primitive metric declaration. Events and service-derived metrics are invalid inference inputs. SDK type systems should enforce this before extraction; registry validation and the Decision API must enforce it again against registered signal declarations.

`MetricObjective.signal` is structurally serialized as a signal key, but it must resolve to a numeric metric declaration. The metric may be app-emitted or derived; events and boolean/string metrics are invalid objectives. SDKs should expose a branded numeric metric identity, and registry/API validation must enforce the same rule.

Metric objective direction is discriminated: `target` requires a finite numeric `target`, while `minimize` and `maximize` must not carry a `target` field. Canonical validation rejects both missing-target and unexpected-target forms.

`DecisionDefinition.policy` is required. Canonicalization never inserts an implicit environment or default policy when it is absent. Code-first shorthand must normalize to `InlinePolicy`; explicit definitions must supply either `PolicyReference` or `InlinePolicy`.

## Policy contract

```ts
type PolicyConstraint =
  | NumberBoundsConstraint
  | MaxDeltaConstraint
  | EvidenceQualityConstraint
  | ModelUncertaintyConstraint
  | ExpectedOutcomeConstraint
  | SampleSizeConstraint;

type NumberBoundsConstraint = {
  kind: "number-bounds";
  min: number;
  max: number;
};

type MaxDeltaConstraint = {
  kind: "max-delta";
  value: number;
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

type PolicyEvaluationResult = {
  result: "approved" | "blocked" | "fallback";
  reasons: string[];
  appliedConstraints: string[];
};
```

The replacement Phase 3 contract intentionally has no generic cooldown,
pause, or other temporal/operator constraint. Follow-up contracts must define
those semantics, required state, and concurrency behavior before they become
shared or authorable surfaces. Issue #33 owns the temporal portion.

Rules:

- Policy reason codes should be stable strings.
- `appliedConstraints` reports every enforced action-space and policy check.
  Labels such as `number-bounds` and `step` may therefore appear even when
  bounds and step come from the output contract rather than `InlinePolicy`.
- Runtime should return fallback when policy result is `fallback`.
- Runtime should not return a candidate value as approved when policy result is `blocked`; a governed fallback response may preserve `blocked` as the policy result.
- Policy and evidence outcomes never authorize SDK-local fallback. The SDK
  availability fallback classifier is limited to genuine data-plane
  availability failures after readiness checks pass.

## Decision strategy

```ts
type NumericRuleStrategyDeclaration = {
  kind: "numeric-rule";
} & NumericRuleDeclaration;

type DecisionStrategyDeclaration = NumericRuleStrategyDeclaration;

type NumericRuleStrategy = NumericRuleStrategyDeclaration & {
  id: string;
};

type DecisionStrategy = NumericRuleStrategy;

type NumericRuleInput = {
  signal: AppEmittedNumericMetricRef;
  minimum: number;
  maximum: number;
  weight: number;
};
```

Rules:

- Strategy execution must be bounded and deterministic in the online runtime path.
- Strategies must not produce values outside the contract action space.
- Candidates and proposals carry `DecisionStrategyDeclaration`, never a
  trusted strategy ID. Activation derives the opaque materialized
  `DecisionStrategy.id` in a distinct namespace from the activation ID and
  canonical strategy declaration, persists it in state, and returns the same
  ID on exact replay.
- Fixed authority is represented only by `DecisionState.activeValue`; it is
  not wrapped in a strategy.
- `NumericRuleStrategy` is the only adaptive strategy required for the MVP.
- Future strategy types should extend both the declaration and materialized
  strategy types without changing `DecideResponse`.

## Decision state

```ts
type DecisionStateCommon = {
  stateId: string;
  proposalId: string;
  activationId: string;
  definition: DecisionDefinitionRef;
  contractDigest: string;
  controlTarget: DecisionTargetRef;
  generation: number;
  predecessorStateId?: string;
  approvalReference: string;
  activatedAt: string;
  lifecycle: "active" | "superseded";
};

type DecisionState =
  | (DecisionStateCommon & {
      authorityKind: "active-value";
      activeValue: DecisionValue;
      activeStrategy?: never;
    })
  | (DecisionStateCommon & {
      authorityKind: "numeric-rule";
      activeValue?: never;
      activeStrategy: NumericRuleStrategy;
    });
```

Rules:

- `DecisionState` owns live runtime authority; `DecisionDefinition` owns declared semantics.
- Bundle-approved and proposal-managed authority converge on this same state
  shape and activation boundary.
- Each state's authority payload and definition binding are immutable.
  `lifecycle` is a read projection: the record at the current head is active
  and retained predecessor records are superseded.
- Exactly one authority payload is valid. `active-value` requires
  `activeValue`; `numeric-rule` requires `activeStrategy`; both-present,
  neither-present, or discriminator/payload mismatch fails readiness.
- Decision API orchestration resolves `activeValue` directly. Only coherent
  `numeric-rule` authority crosses the strategy-executor boundary; state
  absence and policy fallback do not.
- The mutable authority head is keyed by stable application, environment,
  decision key, and control target, not by semantic revision.
- Activation compare-and-swaps that stable head across revisions, increments
  its generation, supersedes the predecessor, and supports idempotent replay.
- Pause, override, completion, expiry, rollback, and temporal state require
  explicit follow-up contracts.

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
  evidenceQuality: number;
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
- Every confidence field is a finite number in the inclusive range `[0, 1]`.
- `evidenceQuality` is required, so an empty confidence object is invalid.
- Higher `evidenceQuality` and `expectedOutcome` are better; higher `modelUncertainty` means less certainty.
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

type DecisionValuePayload =
  | { valueType: "boolean"; value: boolean }
  | { valueType: "number"; value: number }
  | { valueType: "string"; value: string };

type ExposureDirective =
  | {
      confirmationRequired: true;
      confirmToken: string;
    }
  | {
      confirmationRequired: false;
      confirmToken?: never;
    };

type ServerDecisionCommon = {
  decisionKey: string;
  definition: DecisionDefinitionRef;
  decisionId: string;
  decisionMode: "active-value" | "strategy" | "fallback";
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
  exposure: ExposureDirective;
  policy: PolicyEvaluationResult;
  definitionStatus: ContractRuntimeStatus;
};

type ServerDecisionResult = ServerDecisionCommon & DecisionValuePayload;

type ClientFallbackCommon = {
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
};

type ClientFallbackResult = ClientFallbackCommon & DecisionValuePayload;

type DecisionResult = ServerDecisionResult | ClientFallbackResult;

type DecideResponse = ServerDecisionResult;

type RuntimeDecisionResult = ServerDecisionResult;

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

- `correlationId` is internal tracing metadata. HTTP adapters source it only
  from `X-Flaggo-Correlation-Id` (or generate it when absent); runtime and
  management JSON request bodies never serialize it.
- The response returns the final concrete value for application code.
- `valueType` discriminates `value`; mismatched pairs and non-finite number values are invalid.
- `decisionMode` explains how the current Phase 3 value was produced without
  exposing internals. Future mechanisms such as experiment assignment require
  a separately approved response-contract extension.
- The server wire response always contains `definition`, `decisionId`, `auditId`, `policy`, verified `definitionStatus`, and `exposure`.
- A local client fallback caused by data-plane unavailability is an SDK-produced `ClientFallbackResult`; it is disabled unless explicitly configured and cannot claim server policy, definition status, decision, audit, or exposure identity.
- A client fallback carries only `expectedContract` as client provenance, after the local call-site digest has matched the accepted runtime binding.
- A 4xx contract/configuration response can never produce `ClientFallbackResult`.
- `confidence` is non-null only when the returned authority makes an
  evidence-backed claim. It is `null` for server decision fallback,
  non-evidence-based active values, and deterministic bundle-authored
  strategies.
- Resolution fallback preserves the returned authority's confidence semantics:
  an evidence-backed broader-target decision retains its confidence, while a
  deterministic broader-target strategy returns `null`.
- `fallback.resolutionFallbackUsed` is true only when an active authority was
  selected from a broader permitted target. It is false when no authority was
  selected and the server returned the registered fallback.
- A `missing_state` fallback that selected no authority omits
  `controlTarget` and `strategyId`, returns no authority target provenance,
  and may retain the attempted permitted targets in `resolutionChain`.
- Client target/cohort claims are context, not authority. `targetProvenance` records whether each effective target was client-claimed, verified, server-derived, or replaced.
- Default runtime responses contain compact confidence and target provenance; full evidence-view references remain in audit records.
- `exposure.confirmToken` is required exactly when `confirmationRequired` is true and forbidden otherwise. The initial `RuntimeDecisionResult` must not include an `exposureId`; exposure identity is created by confirmation.
- A server decision fallback is not exposure-eligible and therefore returns
  `confirmationRequired: false`.
- `definitionStatus.integrity` indicates whether the client expectation matched a registered known contract definition.
- The full definition bundle is not sent with each request; only compact identity is sent.
- Exposure confirmation is idempotent for the same decision/token and creates the first `exposureId` under accepted decision A4.

## Canonical definition normalization and digest

Every authoring surface must normalize into the same language-neutral `DecisionDefinition` before compatibility comparison or hashing. Combined code-first and explicit forms that express the same semantics must produce byte-identical canonical definitions and therefore the same digest.

The complete authority workflow declaration is semantic content. For
bundle-approved definitions, the control target, numeric rule, weighted inputs,
branch values, threshold, and rationale all participate in the definition
digest.

Normalization:

1. Remove all bound runtime values.
2. Convert each bound signal input into its immutable `SignalRef`.
3. Convert each typed target binding into a runtime-context schema entry. The original object property name is the canonical context field name; for example, `sessionId: flaggo.target.session(value)` becomes `sessionId: { type: "string", target: "session" }`.
4. Normalize SDK-specific policy shorthand, such as client-library [`PolicyAuthoring`](../client-library/README.md#policy-authoring-normalization), into canonical `InlinePolicy` constraints.
5. Materialize generated fields such as `signals.allowed` exclusively from semantic role references. A supplied generated allowlist is never an identity input; stale or extra entries are discarded during extraction.
6. Reduce every signal role/reference to immutable `SignalRef { key }`. `schemaDigest` belongs to signal-declaration conflict detection and is excluded from decision-definition identity.
7. Omit undefined fields and normalize equivalent optional/default forms
   according to the contract version.
8. Sort JSON object keys recursively.
9. Reject duplicate signal keys, duplicate context fields, duplicate policy constraint kinds, or conflicting role/schema declarations.
10. Serialize with RFC 8785 JSON Canonicalization Scheme.
11. Compute SHA-256 over the canonical UTF-8 bytes and encode the identity as `sha256:<lowercase-hex>`.

Order-sensitive arrays retain authored order because order changes behavior:

- `targetHierarchy`,
- `inference.fallbackOrder`,
- prioritized objective lists such as `intent.secondary`,
- tuple-like values such as numeric ranges.

Order-insensitive collections are duplicate-free sets and are sorted by immutable signal key:

- `signals.evidence`,
- `signals.guardrails`,
- generated `signals.allowed`,
- canonical `inference.inputs`,
- `lifecycle.initialAuthority.rule.weightedInputs`, sorted by signal key,
- signal declarations in a bundle,
- `InlinePolicy.constraints`, sorted by constraint kind.

For `bundleDigest`, decision definitions are sorted by stable decision key after each definition has been normalized. Duplicate decision keys with different definition digests are a `contract-conflict`; byte-identical duplicates are deduplicated. Duplicates with equal semantic digests but conflicting lineage or owner metadata are rejected rather than selecting the first entry.

Definition metadata such as `definitionId` and `owner`, plus generated `revision`, `contractDigest`, and `schemaDigest` fields, is excluded from `contractDigest`. Bundle-level build/source metadata remains part of `bundleDigest` so the immutable bundle artifact stays distinguishable, while generated signal digests, set ordering, and definition ordering cannot create accidental differences.

Runtime wire `inputs` are also key-sorted for deterministic transport and audit comparison. Duplicate signal keys are invalid; clients and servers must reject them rather than applying first-wins or last-wins behavior.

`output.range` and `output.step` are enforced directly as action-space
invariants. Canonicalization does not synthesize a `number-bounds` policy
constraint from them. An explicitly authored `number-bounds` constraint is
additional semantic policy and therefore changes the digest.

Code-first `output.default` normalizes to canonical `fallback.value`.
Canonicalization does not synthesize a fallback reason that the authoring
surface cannot express.

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

type ContractRuntimeStatus = {
  definitionId: string;
  revision: string;
  contractDigest: string;
  bundleDigest?: string;
  buildId?: string;
  deploymentId?: string;
  integrity: "verified";
  compatibility?: ContractCompatibility;
};

type ContractCompatibility =
  | "identical"
  | "metadata-only"
  | "new-contract-required";
```

Rules:

- Every server `200` repeats the exact accepted `definitionId + revision + contractDigest` and reports only `integrity: "verified"`. Unknown, conflicting, or retired identities are Problem Details errors rather than alternate success states.
- Stable contract/configuration error codes are `missing-contract-identity`,
  `contract-not-registered`, `contract-conflict`, `unknown-decision-key`, and
  `retired-definition`. Stable readiness/integrity codes are
  `definition-not-ready`, `decision-service-not-ready`, and
  `invalid-decision-state`. None can become server or SDK-local fallback.
- Browser-provided definition identity is useful for drift detection, not as a security boundary.
- Multiple builds of the same service may be deployed at the same time. Runtime integrity must be evaluated against the expected definition identity carried by the calling build, not a singular environment-wide bundle.
- `definitionId` is an opaque registry-issued lineage ID and remains stable across approved semantic revisions. It is never a semantic version and clients must not parse it.
- `revision` is an opaque registry-issued runtime revision ID, not semantic versioning or a metadata revision.
- `contractDigest` identifies canonical semantic content. The immutable runtime identity is the complete `{ definitionId, revision, contractDigest }` tuple.
- Metadata-only edits are retained in registry/audit history without changing runtime identity.
- Semantic approval creates a new revision and digest under the same definition lineage. A new definition ID is reserved for a new lineage or explicit fork.
- `bundleDigest` identifies the full submitted bundle. `buildId`, `deploymentId`, and `artifactDigest` identify the workload instance or release that carries the contract expectation.
- Older revisions are accepted only when the request identifies that exact
  registered tuple and lifecycle permits it. Runtime never substitutes another
  revision; if the stable authority head no longer contains compatible state,
  the accepted older request receives the audited server fallback.

## Contract, telemetry, evidence, and state reuse

Flaggo should avoid sharing unsafe learned decision behavior across different contracts, but it should not throw away useful historical observations.

| Layer | Default sharing behavior | Reason |
| --- | --- | --- |
| Raw telemetry observations | Share across definitions with the same application, signal key, and target semantics. | Observations are historical facts, not learned policy. |
| Evidence views | Share only when signal key, target, window, and filters match. | A metric can be reused if it is the same immutable signal viewed the same way. |
| Decision state or active strategy | Bind each immutable record to one exact definition identity; serialize replacement through the stable authority head for the decision key and control target. | Authority approved for one contract must not execute under another, while one CAS order must prevent stale revisions from replacing newer authority. |

Rules:

- Telemetry identity should be stable at the event/signal level so a new definition can reuse existing observations for unchanged inputs.
- Evidence views should be identified by immutable signal key plus target, window, and filters.
- A new proposal-managed definition may start in partial-warm mode: reused
  evidence can contribute immediately, while new signals collect data until
  policy marks them sufficient.
- The authority head is keyed by application, environment, decision key, and
  resolved control target. Immutable state and strategy payloads remain bound
  to one exact definition identity and are never inherited by another
  revision. Future temporal or operator state follows its own explicitly
  approved address contract.
- State migration between definition IDs should be an explicit operator or registry action, not an implicit compatibility rule.

## Proposal-managed authority (Phase 4 extension point)

Phase 4 proposal and governance DTOs are intentionally not frozen by the Phase
3 contract. Issue #25 must define the smallest concrete wire shapes after the
activation-core reduction. The current shared invariants are:

- a scripted or future intelligence producer emits a proposal and never writes
  active state directly;
- proposal kind is distinct from governance disposition;
- governance may authorize activation only through the shared
  expected-baseline boundary;
- online runtime consumes `DecisionState`, never raw proposal text;
- Phase 4 audit extensions are added with that contract rather than
  predeclared here.

## Audit record

```ts
type AuditDecisionResult =
  Omit<ServerDecisionResult, "exposure"> & {
    exposure?: {
      confirmationRequired: boolean;
    };
  };

type AuditStateSummary =
  | {
      authoritySelected: false;
      resolution: "server-fallback";
    }
  | {
      authoritySelected: true;
      authorityKind: "active-value";
      stateId: string;
      generation: number;
      predecessorStateId?: string;
      proposalId: string;
      activationId: string;
      approvalReference: string;
    }
  | {
      authoritySelected: true;
      authorityKind: "numeric-rule";
      strategyId: string;
      stateId: string;
      generation: number;
      predecessorStateId?: string;
      proposalId: string;
      activationId: string;
      approvalReference: string;
    };

type AuditRecord = {
  auditId: string;
  timestamp: string;
  decisionKey: string;
  request: DecideRequest;
  response: AuditDecisionResult;
  contractVersion?: string;
  runtimeTarget?: DecisionTargetRef;
  controlTarget?: DecisionTargetRef;
  evidence?: EvidenceSnapshot;
  stateSummary: AuditStateSummary;
  policy?: PolicyEvaluationResult;
  reason: string;
};
```

Rules:

- Audit records may contain more detail than runtime responses.
- Every server-produced decision audit captures the exact normalized
  `DecideRequest`, including all inference inputs used by successful strategy
  execution. SDK-local fallback produces no server audit record.
- The Decision API preallocates one `auditId` before constructing the response
  or audit record. `AuditRecord.auditId` and `AuditRecord.response.auditId`
  must be identical, and the returned server response uses that same ID.
  `IAuditSink` persists the supplied identity and never mints or replaces it.
- Every server decision carries one `stateSummary`. No-authority fallback
  carries no lineage. Selecting active-value authority requires the complete
  state, proposal, activation, and approval lineage; selecting numeric-rule
  authority requires that lineage plus `strategyId`.
- If policy replaces a selected authority's candidate with server fallback,
  the response records fallback while `stateSummary` preserves the selected
  authority lineage.
- Confirmation tokens are capabilities and must be removed before constructing `AuditRecord`; audit response projections can retain `confirmationRequired` but never `confirmToken`.
- Audit should be local-first in MVP. A ready data plane requires a durable
  file or SQLite sink whose successful `IAuditSink.record` completion means
  the record survives process failure. Console and in-memory sinks are limited
  to tests or explicitly non-ready debugging modes.
- Cloud audit sinks should implement `IAuditSink`; they should not change the audit contract.

## Contract bundle and registration receipt

```ts
type DecisionDefinitionBundle = {
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
  source: {
    repository?: string;
    path?: string;
    commit?: string;
  };
  signals?: SignalDeclaration[];
  definitions: DecisionDefinitionBundleEntry[];
};

type DecisionDefinitionBundleEntry = {
  definitionId?: string;
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
  lifecycle: AuthorityLifecycleDeclaration;
};

type DefinitionBundleValidationResult = {
  status: "valid" | "invalid";
  bundleDigest?: string;
  compatibility?: ContractCompatibility;
  validatedDefinitions?: Record<
    string,
    {
      definitionId?: string;
      revision?: string;
      contractDigest: string;
    }
  >;
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

type ContractChange =
  | {
      kind: "created";
      decisionKey: string;
      proposed: {
        definitionId: string;
        contractDigest: string;
      };
    }
  | {
      kind: "metadata-updated" | "deprecation-candidate";
      decisionKey: string;
    }
  | {
      kind: "authority-reauthorization";
      decisionKey: string;
      definition: DecisionDefinitionRef;
      contractDigest: string;
      controlTarget: DecisionTargetRef;
      previousActivationId: string;
    }
  | {
      kind: "semantic-change";
      decisionKey: string;
      previous: AcceptedDefinition;
      proposed: {
        definitionId: string;
        contractDigest: string;
      };
      semanticDiff: ContractSemanticDiffOperation[];
    };

type CanonicalJsonValue =
  | null
  | boolean
  | number
  | string
  | CanonicalJsonValue[]
  | { [key: string]: CanonicalJsonValue };

type ContractSemanticDiffOperation =
  | { op: "add"; path: string; after: CanonicalJsonValue }
  | { op: "remove"; path: string; before: CanonicalJsonValue }
  | {
      op: "replace";
      path: string;
      before: CanonicalJsonValue;
      after: CanonicalJsonValue;
    };

type AcceptedDefinition = {
  definitionId: string;
  revision: string;
  contractDigest: string;
  activatedAuthority?: ActivatedAuthorityReceipt;
};

type ActivatedAuthorityReceiptCommon = {
  proposalId: string;
  activationId: string;
  stateId: string;
  generation: number;
  controlTarget: DecisionTargetRef;
};

type ActivatedAuthorityReceipt =
  | (ActivatedAuthorityReceiptCommon & {
      kind: "active-value";
      strategyId?: never;
    })
  | (ActivatedAuthorityReceiptCommon & {
      kind: "numeric-rule";
      strategyId: string;
    });

type ExpectedAuthorityBaseline = {
  stateId?: string;
  generation: number;
};

type InitialAuthorityActivationPlan = {
  decisionKey: string;
  definition: DecisionDefinitionRef;
  contractDigest: string;
  proposalId: string;
  activationId: string;
  controlTarget: DecisionTargetRef;
  kind: "active-value" | "numeric-rule";
  expectedBaseline: ExpectedAuthorityBaseline;
};

type RegistrationReceipt = {
  application: string;
  environment: string;
  bundleDigest: string;
  buildId?: string;
  artifactDigest?: string;
  acceptedDefinitions: Record<string, AcceptedDefinition>;
  compatibility: ContractCompatibility;
  status: "ready";
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
      compatibility: "identical" | "new-contract-required";
      expiresAt: string;
      snapshotUrl: string;
      supersedesApprovalRequestId?: string;
      changes: ContractChange[];
      issues: ContractIssue[];
    }
  | {
      status: "activation-pending";
      approvalRequestId: string;
      application: string;
      environment: string;
      bundleDigest: string;
      activations: InitialAuthorityActivationPlan[];
      issues: ContractIssue[];
    }
  | {
      status: "activation-failed";
      approvalRequestId: string;
      application: string;
      environment: string;
      bundleDigest: string;
      activations: InitialAuthorityActivationPlan[];
      retryability: "retryable" | "requires-new-approval";
      issues: ContractIssue[];
    }
  | {
      status: "approval-rejected";
      approvalRequestId: string;
      application: string;
      environment: string;
      bundleDigest: string;
      reasonCode: string;
      issues: ContractIssue[];
    };

type ApprovalActor = {
  subject: string;
  displayName?: string;
};

type DefinitionBundleApprovalCommon = {
  approvalRequestId: string;
  application: string;
  environment: string;
  bundleDigest: string;
  createdAt: string;
  expiresAt: string;
  snapshotUrl: string;
  supersedesApprovalRequestId?: string;
  changes: ContractChange[];
};

type DefinitionBundleApprovalResult =
  DefinitionBundleApprovalCommon &
  (
    | {
        status: "pending";
      }
    | {
        status: "approved";
        decidedAt: string;
        approval: {
          actor: ApprovalActor;
          comment?: string;
        };
        activation:
          | {
              status: "pending";
              activations: InitialAuthorityActivationPlan[];
            }
          | {
              status: "ready";
              receipt: RegistrationReceipt;
            }
          | {
              status: "failed";
              activations: InitialAuthorityActivationPlan[];
              retryability: "retryable" | "requires-new-approval";
              issues: ContractIssue[];
            };
      }
    | {
        status: "rejected";
        decidedAt: string;
        rejection: {
          actor: ApprovalActor;
          reasonCode: string;
          comment?: string;
        };
      }
    | {
        status: "expired";
        expiredAt: string;
      }
  );

```

Rules:

- `DecisionDefinitionBundle` is the canonical language-neutral sync artifact.
- SDK-generated declarations, hand-authored JSON/YAML, GitOps workflows, and registry exports should all produce or reference the same bundle shape.
- Bundle sync validates submitted semantics and compares them with accepted
  revisions.
- Apply of a new key reserves and persists its proposed lineage ID and
  contract digest in the approval request. Successful approval allocates and
  publishes the initial opaque runtime revision.
- A bundle may omit `definitionId` for a new decision key. Apply reserves the
  registry-assigned opaque lineage ID in the approval request; clients never
  synthesize version-bearing IDs.
- When an approval request creates a definition, its `created` change persists
  the server-allocated `definitionId` and `contractDigest`. Exact retry reuses
  that proposed identity and never allocates another lineage for the same
  approval snapshot.
- Omitted `definitionId` resolves the existing lineage for a known key. A supplied ID must already belong to that same authorized application/environment/key; unknown or mismatched IDs are validation errors.
- Missing bundle resources become deprecation candidates, not deletes.
- Bundle data must remain provider-neutral.
- Build metadata is allowed in the bundle for traceability, but compatibility should be based on canonical definition content, not incidental build metadata. Two different builds with identical decision definitions may share the same `contractDigest` while having different `buildId` or `artifactDigest`.
- `acceptedDefinitions` provides the complete per-key runtime binding and any
  required activated-authority references. A client must initialize each call
  site from its receipt entry rather than combine a top-level digest with a
  revision map.
- Registry-managed revisioning is the default UX. Metadata-only changes keep the same runtime identity; approved semantic changes mint a new opaque revision and digest under the same definition lineage.
- Bundle validation issues use stable machine-readable codes and JSON Pointer paths; clients must not parse prose messages.
- Bundle validation and immutable definition publication are atomic. A
  semantic change returns `requires-approval` without active-authority
  mutation. Explicit approval is durably recorded before any required
  expected-baseline activation. Bundle-approved registration remains non-ready
  until all required activation succeeds; proposal-managed approval publishes
  no initial activation plan and can store the ready receipt immediately.
- The `pending -> approved` transition atomically persists one
  `InitialAuthorityActivationPlan` per required bundle-approved authority.
  Each plan binds the deterministic proposal and activation IDs to the stable
  authority-head baseline observed at that transition. No-state baselines use
  generation `0` with no `stateId`.
- Activation and every exact retry use the persisted `expectedBaseline`; they
  never re-read the head and substitute a later baseline. A head advanced
  after approval therefore produces a stale-baseline conflict and
  `requires-new-approval` rather than overwriting newer authority.
- For each bundle-approved definition, proposal and activation IDs are
  server-derived in distinct namespaces from application, environment,
  approval request ID, decision key, contract digest, and canonical initial
  authority. Exact replay returns those IDs and the original state identity;
  callers cannot supply or recompute them as authority.
- Numeric-rule activation derives the strategy ID in a third namespace from
  the activation ID and canonical ID-free strategy declaration. It persists
  that identity in the active state and ready receipt; exact replay returns the
  same strategy ID. Active-value activation persists the approved value
  directly and its receipt forbids `strategyId`.
- `activation-failed` with `retryability: "retryable"` represents interruption
  or outcome uncertainty. Exact apply resumes the same activation and first
  resolves any already-published state before attempting publication again.
  `requires-new-approval` represents a stale expected baseline or permanent
  conflict. The failed approval remains non-ready. The next exact reapply
  atomically advances the same deterministic registration attempt to one
  linked `requires-approval` result; concurrent reapplies converge on that
  successor.
- `activation-pending.activations` contains only still-pending plans.
  `activation-failed.activations` contains only failed or unresolved plans,
  and its non-empty `issues` identify those plans by `decisionKey`; successful
  partial activations remain durable but are not runtime bindings until the
  complete ready receipt exists. `requires-new-approval` dominates the
  aggregate retryability when any failed plan requires reauthorization.
- A permanent-failure successor contains `authority-reauthorization` changes
  only for failed initial authorities. It reuses the already published
  definition revision and every successful partial activation, but allocates a
  new approval request and deterministic activation identity whose baseline is
  captured from the current stable head. It never fabricates another semantic
  revision for the unchanged bundle. Its ready receipt contains the complete
  bundle binding, combining retained successful entries with reauthorized
  entries.
- That successor has `compatibility: "identical"` because canonical semantics
  did not change; its approval requirement authorizes the new captured
  baseline and activation, not a contract revision.
- A `requires-approval` result uses `new-contract-required` when any change is
  `created` or `semantic-change`; it uses `identical` only when every change is
  `authority-reauthorization`.
- Replaying a rejected approval returns `approval-rejected`; rejection never
  produces a ready receipt or SDK/runtime binding.
- Approval requests are immutable snapshots with an authoritative expiration.
  The approval decision transitions once to approved, rejected, or expired and
  never changes. Retryable activation progress may continue under the same
  approved snapshot until it is ready or requires a new approval.
- Approval changes include the server-allocated proposed identity for created
  definitions, previous/proposed contract digests for semantic changes, and a
  canonical semantic diff. `snapshotUrl` retrieves the immutable canonical
  bundle under review.
- Snapshot HTTP responses quote the project `sha256:<hex>` value as an opaque `ETag` and separately encode the raw SHA-256 bytes using RFC 9530 `Content-Digest: sha-256=:<base64>:` syntax.
- Approval/rejection records persist the server-derived actor and submitted comment. Expired apply attempts may be resubmitted with the same deterministic key; revalidation creates one linked replacement request.

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
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "status": "active",
  "valueType": "number",
  "actionSpace": {
    "type": "number",
    "min": 200,
    "max": 1500,
    "step": 50,
    "default": 800
  },
  "fallback": {
    "value": 800
  },
  "runtimeContextSchema": {
    "userId": { "type": "string", "target": "user" },
    "sessionId": { "type": "string", "target": "session" },
    "cohort": { "type": "string", "target": "cohort" }
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
    ],
    "evidence": [
      { "key": "tetris.earlyLossRate24h" },
      { "key": "tetris.hardDropRate24h" },
      { "key": "tetris.piecePlaced" },
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
  "lifecycle": {
    "authorityMode": "bundle-approved",
    "initialAuthority": {
      "controlTarget": {
        "type": "cohort",
        "id": "new_players"
      },
      "kind": "numeric-rule",
      "rule": {
        "threshold": 0.55,
        "valueAtOrAbove": 850,
        "valueBelow": 750,
        "weightedInputs": [
          {
            "signal": { "key": "tetris.boardPressure" },
            "minimum": 0,
            "maximum": 1,
            "weight": 0.45
          },
          {
            "signal": { "key": "tetris.recentPlacementTimeMs" },
            "minimum": 0,
            "maximum": 2000,
            "weight": 0.25
          },
          {
            "signal": { "key": "tetris.recoveryFailures" },
            "minimum": 0,
            "maximum": 5,
            "weight": 0.2
          },
          {
            "signal": { "key": "tetris.currentLevel" },
            "minimum": 0,
            "maximum": 20,
            "weight": 0.1
          }
        ]
      },
      "rationale": "Initial deterministic Tetris behavior."
    }
  },
  "policy": {
    "kind": "inline",
    "constraints": [
      { "kind": "max-delta", "value": 50 }
    ]
  }
}
```

## MVP domain and wire contract direction

The bundle v1 freeze is superseded by the approved bundle-approved authority
design. The contract-first implementation must update schema, generated types,
canonical fixtures, SDK and service conformance together and remove the v1 path
rather than supporting both formats.

Retain these provider-neutral domain boundaries:

- result value primitives,
- action space shapes,
- `DecisionTargetRef`,
- `DecisionDefinition`,
- fixed authority through `DecisionState.activeValue`,
- `DecisionStrategy` for the `numeric-rule` runtime mechanism,
- `DecisionState`,
- `PolicyEvaluationResult`,
- `AuditRecord`,
- `DecisionDefinitionBundle`,
- `ContractIdentity`,
- `ContractRuntimeStatus`,
- `ContractCompatibility`.

The runtime decision and exposure projections remain governed by the accepted
[Phase 1 API Contract Proposal](../API_CONTRACT_PROPOSAL.md) and executable
conformance artifacts. The bundle management, approval, and registration
receipt projections are intentionally replaced by the approved v2 direction;
the follow-up contract issue must update all executable artifacts together.
