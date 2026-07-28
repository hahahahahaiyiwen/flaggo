# MVP Implementation Guide

## Purpose

This guide defines the implementation shape for the first Flaggo MVP. The goal is to keep interfaces simple enough to build, but extensive enough that the MVP proves the core primitive and can evolve without rewriting module boundaries.

The MVP should prove one end-to-end decision:

```text
tetris.dropInterval
```

The decision should support real-time adaptation:

```text
async intelligence proposes a bounded strategy
  -> governance activates it
  -> online runtime executes it against live game context
  -> client receives an immediate dropInterval value
```

## MVP boundaries

The first implementation should include:

| Area | MVP expectation |
| --- | --- |
| Client library | TypeScript SDK for declaring the Tetris decision, emitting telemetry, and calling the runtime API. |
| Decision API | Runtime endpoint that validates request, resolves targets, executes active value or active strategy, applies governance, audits, and returns a value. |
| Definition registry | Minimal in-memory or simple persistent definition lookup for registered decision keys. |
| Evidence | Minimal evidence snapshot abstraction; real aggregation can be simple at first. |
| State | Active value or active strategy per decision definition and target. |
| Policy | Deterministic bounds, max delta, cooldown, fallback, and evidence/model confidence handling. |
| Audit | Structured audit record with correlation ID. |
| Async intelligence | Can start as manual or scripted strategy proposal generation, but must use the same proposal and strategy interfaces future agents will use. |

The MVP should not require:

- a full operator console,
- a full experiment platform,
- general-purpose LLM orchestration,
- multi-tenant enterprise governance,
- complex model training,
- production-grade telemetry storage.

## Interface principles

1. **Interfaces at module boundaries**: domain behavior depends on ports, not concrete storage, telemetry, or model providers.
2. **Primitive result values**: runtime decisions return only `boolean`, `number`, or `string`.
3. **Strategies are explicit contracts**: adaptive behavior is represented as a strategy object, not hidden application `if/else`.
4. **Governance is downstream and mandatory**: a value or strategy is not runtime-authoritative until policy and state allow it.
5. **Runtime stays bounded**: online execution should use active state, approved strategies, and fast evidence; deep analysis belongs to async intelligence.
6. **Extensibility through discriminated unions**: new strategy types, evidence sources, and proposal types should extend explicit unions instead of changing every call shape.
7. **Audit every decision**: every runtime response should be reconstructable from definition revision, target, state, strategy, evidence, policy, and audit ID.

## Open-source native and cloud portability principles

Flaggo should be open-source native first, with clean portability to cloud provider services. The core project should run locally without a managed cloud dependency, while exposing interfaces that make cloud-backed implementations straightforward.

MVP design rules:

1. **Local-first runtime**: the MVP should run with a simple local setup such as process memory, local files, SQLite, or a containerized database.
2. **Provider-neutral core**: domain and API logic must not depend directly on Azure, AWS, Google Cloud, or any proprietary service SDK.
3. **Standard protocols first**: prefer OpenTelemetry for telemetry, OpenAPI for REST contracts, CloudEvents-compatible event shapes where useful, and container images for deployment.
4. **Cloud adapters at the edge**: cloud-specific integrations should implement ports such as `IStateStore`, `IEvidenceProvider`, `IAuditSink`, and `ISecretProvider`.
5. **Portable configuration**: use environment variables and explicit configuration files; avoid hidden cloud environment assumptions.
6. **Swappable persistence**: start with in-memory or SQLite-like stores, but keep the boundary compatible with PostgreSQL, cloud SQL, document stores, or object storage.
7. **No proprietary lock-in in contracts**: public API, manifest, strategy, policy, and audit schemas should be usable without a cloud account.
8. **Cloud reference deployments later**: Azure, AWS, and GCP deployment templates can be added as optional adapters and recipes, not as the only way to run Flaggo.

Portability target:

```text
open-source core
  -> local/dev adapters
  -> container deployment
  -> optional cloud provider adapters
```

The MVP should optimize for a contributor being able to clone the repository, run the server, run the Tetris demo, inspect audit output, and understand the system without provisioning cloud infrastructure.

## Portability interfaces

Cloud portability should be designed through explicit ports. These ports can start with local implementations and later gain cloud implementations.

```ts
interface IConfigProvider {
  getString(key: string): string | undefined;
  getNumber(key: string): number | undefined;
  getBoolean(key: string): boolean | undefined;
}

interface ISecretProvider {
  getSecret(name: string): Promise<string | undefined>;
}

interface IClock {
  now(): Date;
}

interface IIdGenerator {
  newId(prefix?: string): string;
}

interface IHealthReporter {
  getHealth(): Promise<HealthStatus>;
}
```

Cloud-backed implementations should be additive:

| Port | Local MVP implementation | Future cloud implementation |
| --- | --- | --- |
| `IDefinitionRegistry` | in-memory or local JSON/SQLite | PostgreSQL, Azure SQL, DynamoDB, Firestore |
| `IStateStore` | in-memory or SQLite | Redis, Cosmos DB, DynamoDB, Cloud SQL |
| `IEvidenceProvider` | in-memory snapshots or local aggregation | OpenTelemetry pipeline, metrics store, data warehouse |
| `IAuditSink` | console/file/SQLite | object storage, event hub, managed logging |
| `ISecretProvider` | environment variables | Key Vault, Secrets Manager, Secret Manager |
| `IConfigProvider` | environment variables/local config | App Configuration, Parameter Store, Config Controller |

The implementation should prove the interface shape before optimizing any cloud adapter.

## Shared contract types

The canonical MVP interface freeze is documented in [Shared Contracts](design/shared-contracts/README.md). The shapes below summarize the core runtime types that SDK and API implementation should share.

```ts
type ValueType = "boolean" | "number" | "string";
type DecisionValue = boolean | number | string;
type RuntimeContextValue = boolean | number | string | null;
type RuntimeContext = Record<string, RuntimeContextValue>;

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

type ActionSpace = NumberActionSpace | BooleanActionSpace | StringActionSpace;
```

## Decision strategy interface

The MVP should support a fixed value strategy and a simple numeric rule strategy. The shape should remain extensible for future scoring functions, small models, bandits, and experiment assignment.

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

For the Tetris MVP, the active strategy can be:

```json
{
  "kind": "numeric-rule",
  "baseValue": 800,
  "min": 600,
  "max": 1100,
  "step": 50,
  "cooldownSeconds": 20,
  "rules": [
    {
      "id": "slow-down-under-pressure",
      "when": {
        "all": [
          { "fact": "boardPressure", "operator": "gte", "value": 0.7 },
          { "fact": "recentPlacementTimeMs", "operator": "gte", "value": 1200 }
        ]
      },
      "adjustBy": 50,
      "reason": "Player is under board pressure and placing slowly."
    }
  ]
}
```

## Runtime request and response interfaces

```ts
type DecideRequest = {
  decisionKey: string;
  runtimeTarget?: DecisionTargetRef;
  runtimeContext: RuntimeContext;
  client: {
    appId: string;
    environment: string;
    sdk?: string;
    sdkVersion?: string;
  };
  correlationId?: string;
};

type DecideResponse<T extends DecisionValue = DecisionValue> = {
  decisionKey: string;
  definition?: DecisionDefinitionRef;
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
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: boolean;
    reason: string | null;
  };

  type ConfidenceReport = {
    evidenceQuality?: number;
    modelUncertainty?: number;
    expectedOutcome?: number;
  };
  policy: {
    result: "approved" | "blocked" | "fallback";
    reasons: string[];
    appliedConstraints: string[];
  };
};
```

## Client library interfaces

The TypeScript SDK should expose a small public API:

```ts
interface IFlaggoClient {
  tune: ITuneBuilder;
  events: IEventBuilder;
  metrics: IMetricBuilder;
  definitions: IDefinitionBundleProvider;
}

interface ITuneBuilder {
  number(name: string, request: NumberTuneRequest): Promise<number>;
  numberDetailed(name: string, request: NumberTuneRequest): Promise<DecideResponse<number>>;
}

interface IDefinitionBundleProvider {
  exportBundle(): DecisionDefinitionBundle;
  getExpectedIdentity(): ContractIdentity | undefined;
}
```

The SDK should not own decision intelligence, policy, or state. It should declare decision definitions, optionally export a canonical definition bundle, send runtime context and compact definition identity, emit telemetry, and expose typed responses.

## Decision API service interfaces

The Decision API should be implemented by composing explicit ports:

```ts
interface IDecisionService {
  decide(request: DecideRequest): Promise<DecideResponse>;
}

interface IDefinitionRegistry {
  getActiveDefinition(ref: DecisionDefinitionRef): Promise<DecisionDefinition>;
  validateBundle(bundle: DecisionDefinitionBundle): Promise<DefinitionBundleValidationResult>;
  applyBundle(bundle: DecisionDefinitionBundle): Promise<RegistrationReceipt>;
}

interface ITargetResolver {
  resolve(request: DecideRequest, definition: DecisionDefinition): Promise<ResolvedTargets>;
}

interface IEvidenceProvider {
  getSnapshot(input: EvidenceRequest): Promise<EvidenceSnapshot>;
}

interface IStateStore {
  getActiveState(input: StateRequest): Promise<DecisionState | null>;
  updateActiveState(input: StateUpdate): Promise<void>;
}

interface IStrategyExecutor {
  execute(input: StrategyExecutionRequest): Promise<StrategyExecutionResult>;
}

interface IPolicyEvaluator {
  evaluate(input: PolicyEvaluationRequest): Promise<PolicyEvaluationResult>;
}

interface IAuditSink {
  record(input: AuditRecord): Promise<{ auditId: string }>;
}
```

Port request/result DTOs such as `EvidenceRequest`, `StateRequest`, `StrategyExecutionRequest`, and `PolicyEvaluationRequest` are owned by their component seams and summarized in the component design docs. Shared cross-component contracts stay in [Shared Contracts](design/shared-contracts/README.md).

The online Decision API flow should compose these ports:

```text
DecideRequest
  -> IDefinitionRegistry.getActiveDefinition
  -> ITargetResolver.resolve
  -> IEvidenceProvider.getSnapshot
  -> IStateStore.getActiveState
  -> IStrategyExecutor.execute or fixed active value
  -> IPolicyEvaluator.evaluate
  -> IAuditSink.record
  -> DecideResponse
```

## Async intelligence interfaces

The MVP can keep async intelligence simple, but it should use future-compatible interfaces.

```ts
type DecisionProposal =
  | ValueProposal
  | StrategyProposal
  | ExperimentProposal
  | HoldProposal
  | RollbackProposal;

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

interface IDecisionIntelligence {
  propose(input: IntelligenceRequest): Promise<DecisionProposal>;
}

interface IProposalGovernance {
  review(proposal: DecisionProposal): Promise<GovernanceOutcome>;
}
```

For MVP, a script, fixture, or admin action can create the Tetris strategy proposal. The important design constraint is that the online path consumes the same governed `DecisionStrategy` representation future AI agents will produce.

## MVP Tetris flow

```text
1. Definition sync
   Register tetris.dropInterval as number, 200-1500ms, step 50, fallback 800.

2. Strategy activation
   Activate a governed numeric-rule strategy for session/new-player targets.

3. Runtime decision
   Game calls POST /v1/decisions/tetris.dropInterval:decide with live context:
     boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel.

4. Strategy execution
   Decision API loads active strategy and calculates the immediate value.

5. Governance
   Policy checks min/max, step, max delta, cooldown, pause/override, fallback.

6. Response
   API returns value, decisionMode=strategy, strategyId, targets, policy result, reason, auditId.

7. Feedback
   Client emits outcome telemetry for later evidence and async intelligence.
```

## MVP design and implementation plan

The MVP should proceed in phases. Each phase should leave behind a working, inspectable slice rather than only abstract design.

### Phase 0: Repository and contributor baseline

Goal: make Flaggo easy to run and understand as an open-source project.

Deliverables:

- root README with product framing and quickstart,
- clear repository layout for SDK, server, shared contracts, and demo,
- local development command,
- container-friendly server startup,
- example environment configuration,
- license and contribution expectations when the project is ready to publish.

Decision rule: if a new contributor cannot run the MVP locally without cloud setup, the foundation is not portable enough.

### Phase 1: Shared definitions, definition bundle, and receipt

Goal: define the stable shapes that SDK, server, tests, and future adapters share.

Deliverables:

- `DecisionValue`, `ActionSpace`, `DecisionTargetRef`, `DecideRequest`, and `DecideResponse`,
- `DecisionStrategy` with `fixed-value` and `numeric-rule`,
- `DecisionProposal` with at least `StrategyProposal`,
- canonical `DecisionDefinitionBundle` schema,
- deterministic bundle and definition digests,
- `RegistrationReceipt`,
- compact definition identity fields for runtime,
- `definition.integrity` response shape,
- JSON examples for `tetris.dropInterval`.

Validation:

- schema examples round-trip successfully,
- invalid strategy/action-space combinations are rejected,
- definition bundle can represent the Tetris decision key without cloud-specific fields,
- direct REST clients can carry expected definition/build identity without any SDK.

### Phase 2: Decision API core with local adapters

Goal: implement the online runtime path behind provider-neutral interfaces.

Deliverables:

- `POST /v1/decisions/{decisionKey}:decide`,
- runtime definition identity verification,
- `IDefinitionRegistry` local implementation,
- `ITargetResolver`,
- `IStateStore` local implementation,
- `IEvidenceProvider` local implementation,
- `IStrategyExecutor` for numeric rules,
- `IPolicyEvaluator` for min/max, step, max delta, cooldown, pause, and fallback,
- `IAuditSink` local implementation,
- health endpoint and structured logs.

Validation:

- fixed fallback response works when no active strategy exists,
- active numeric rule strategy returns adaptive values from runtime context,
- policy blocks out-of-range and cooldown-violating candidates,
- every response includes `auditId`, `decisionMode`, target fields, fallback fields, and definition integrity status.

### Phase 3: TypeScript client library

Goal: make application integration simple while keeping server authority.

Deliverables:

- `createFlaggoClient`,
- `decision.number(...)`,
- `decide(...)`,
- local fallback behavior when the server is unavailable,
- domain event definition and emit API,
- definition bundle export or reference,
- expected definition digest/revision propagation,
- OpenTelemetry-compatible telemetry mode stub or first implementation.

Validation:

- Tetris code can declare `tetris.dropInterval`,
- Tetris code can call `decide` with live context,
- SDK exposes typed `DecisionResult<number>`,
- SDK does not mutate production management state at runtime,
- runtime calls send compact identity rather than the full bundle.

### Phase 4: Tetris adaptive demo

Goal: prove the hero scenario end to end.

Deliverables:

- Tetris emits live context such as `boardPressure`, `recentPlacementTimeMs`, and `recoveryFailures`,
- server has active `numeric-rule` strategy for `tetris.dropInterval`,
- game applies returned `dropInterval`,
- audit output shows strategy execution and policy result,
- fallback behavior is visible when server or policy blocks decisioning.

Validation:

- under high pressure and slow placement, interval slows within max delta,
- after recovery, interval stabilizes or speeds up within bounds,
- cooldown prevents chaotic changes,
- fallback remains `800ms`.

### Phase 5: Minimal async intelligence loop

Goal: introduce the async path without requiring a full AI platform.

Deliverables:

- scripted or fixture-based strategy proposal generation,
- `IProposalGovernance.review(...)`,
- strategy activation flow,
- audit record for proposal review and activation,
- documentation showing where future AI agents plug in.

Validation:

- a strategy proposal can be reviewed and activated,
- rejected proposals do not affect runtime state,
- activated strategies are consumed by the same online runtime path.

### Phase 6: Portable deployment and cloud adapter readiness

Goal: make the MVP portable before adding provider-specific integrations.

Deliverables:

- container image or container-ready startup,
- local compose-style deployment if needed,
- OpenAPI document for the runtime API,
- OpenTelemetry collector-compatible configuration,
- adapter interface documentation,
- one documented path for a future cloud-backed adapter.

Validation:

- local run still works with no cloud services,
- cloud-specific code remains behind adapters,
- API and manifest contracts remain provider-neutral.

## Recommended MVP build sequence

1. Finish shared contracts and examples first.
2. Build the Decision API with local in-memory adapters.
3. Add numeric rule strategy execution and policy governance.
4. Build the TypeScript SDK decision call.
5. Wire Tetris to the SDK and server.
6. Add local telemetry/audit visibility.
7. Add definition bundle export/reference and validate/apply flow.
8. Add scripted strategy proposal activation.
9. Package local startup and document quickstart.
10. Add optional cloud adapter designs only after the local MVP is stable.

## Extensibility checkpoints

Before adding features, verify the change extends one of these seams instead of bypassing it:

- new result shape: should not be added unless boolean/number/string is insufficient,
- new strategy type: extend `DecisionStrategy`,
- new evidence source: implement `IEvidenceProvider`,
- new storage backend: implement `IDefinitionRegistry` or `IStateStore`,
- new policy rule: extend `IPolicyEvaluator`,
- new async reasoning mode: implement `IDecisionIntelligence`,
- new telemetry transport: extend SDK telemetry mode without changing decision calls,
- new cloud provider: implement adapters behind existing ports instead of changing core contracts.
