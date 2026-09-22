# MVP Implementation Guide

## Purpose

This guide defines the implementation shape for the first Flaggo MVP. The goal is to keep interfaces simple enough to build, but extensive enough that the MVP proves the core primitive and can evolve without rewriting module boundaries.

The MVP should prove one end-to-end decision:

```text
tetris.dropInterval
```

The decision should support real-time adaptation:

```text
definition bundle declares bounded initial authority
  -> authenticated approval authorizes the exact snapshot
  -> shared activation publishes governed state
  -> online runtime executes it against live game context
  -> client receives an immediate dropInterval value
```

Phase 4 later adds independently generated proposals through the same
activation boundary.

## MVP boundaries

The first implementation should include:

| Area | MVP expectation |
| --- | --- |
| Client library | TypeScript SDK for declaring the Tetris decision, emitting telemetry, and calling the runtime API. |
| Decision API | Runtime endpoint that validates request, resolves targets, executes active value or active strategy, applies governance, audits, and returns a value. |
| Definition registry | Minimal in-memory or simple persistent definition lookup for registered decision keys. |
| Evidence | Optional runtime evidence abstraction retained for authorities that explicitly require it; the bundle-approved Tetris rule does not. |
| State | One stable authority head per application/environment/decision key/control target, with immutable exact-definition state records carrying the active value or strategy, activation identity, generation, predecessor, and approval lineage. |
| Policy | Deterministic bounds, step, max delta, fallback, and conditional evidence/model handling. Temporal stabilization remains separately specified. |
| Audit | Structured audit record with correlation ID. |
| Async intelligence | Deferred to Phase 4; it must produce proposals for the same governed activation boundary. |

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
5. **Runtime stays bounded**: online execution should use active state,
   approved strategies, and only evidence explicitly required by the active
   authority; deep analysis belongs to async intelligence.
6. **Extensibility through discriminated unions**: new strategy types, evidence sources, and proposal types should extend explicit unions instead of changing every call shape.
7. **Audit every decision**: every runtime response should be reconstructable
   from definition revision, target, state, strategy, applicable evidence,
   policy, and audit ID.

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
| `IRuntimeDefinitionReader` / `IIntelligenceDefinitionReader` | in-memory or local JSON/SQLite | PostgreSQL, Azure SQL, DynamoDB, Firestore |
| `IStateStore` | in-memory or SQLite | Redis, Cosmos DB, DynamoDB, Cloud SQL |
| `IEvidenceProvider` | in-memory snapshots or local aggregation | OpenTelemetry pipeline, metrics store, data warehouse |
| `IAuditSink` | console/file/SQLite | object storage, event hub, managed logging |
| `ISecretProvider` | environment variables | Key Vault, Secrets Manager, Secret Manager |
| `IConfigProvider` | environment variables/local config | App Configuration, Parameter Store, Config Controller |

The implementation should prove the interface shape before optimizing any cloud adapter.

## Contract and API sources of truth

The implementation guide does not redefine shared DTOs. These documents and generated artifacts are authoritative:

| Surface | Source of truth |
| --- | --- |
| Language-neutral domain and runtime contracts | [Shared Contracts](design/shared-contracts/README.md) |
| Runtime HTTP behavior | [Decision API Design](design/decision-api/README.md) and the versioned OpenAPI document |
| Phase 1 endpoint/artifact boundary and accepted decisions | [API Contract Proposal](design/API_CONTRACT_PROPOSAL.md) |
| SDK authoring and runtime UX | [Client Library Design](design/client-library/README.md) |
| Code-first control-plane/data-plane lifecycle | [Control Plane and Data Plane UX](design/client-library/CONTROL_DATA_PLANE_UX.md) |
| Definition bundle and canonical digest rules | [Contract Registry Design](design/contract-registry/README.md) |
| Strategy execution | [Reasoning Engine Design](design/reasoning-engine/README.md) |
| Component-owned ports | The corresponding [component design folder](design/README.md) |

The API-contract phase produced version-controlled artifacts before client or
service implementation branches:

The [API Contract Proposal](design/API_CONTRACT_PROPOSAL.md) and the accepted
OpenAPI, JSON Schema, fixtures, conformance tests, and mock server under
`contracts/` are the shared implementation baseline.

```text
contracts/
  openapi/
    flaggo-runtime-v1.yaml
    flaggo-management-v1.yaml
  schemas/
    decision-definition-bundle-v1.schema.json
  fixtures/
    runtime/
      decide/
      exposure-confirmation/
    management/
      definition-bundle/
    errors/
```

Required API surfaces:

- `POST /v1/decisions/{decisionKey}:decide`,
- `POST /v1/exposures/{decisionId}:confirm`,
- definition-bundle validate/apply management operations,
- health/readiness endpoint,
- stable error and fallback representations.

Golden fixtures must cover:

- approved active value,
- approved strategy execution,
- server policy fallback,
- client fallback representation,
- unknown and conflicting definition identity,
- invalid/duplicate inference inputs,
- exposure confirmation,
- definition validation success and failure.

Both SDK and service tests consume the same schemas and fixtures. Neither branch may maintain an independent copy of `DecideRequest`, `RuntimeDecisionResult`, `DecisionDefinitionBundle`, policy, confidence, fallback, or exposure contracts.

## Control-plane and data-plane implementation boundary

- Definition-bundle validate/apply belongs to the control plane.
- Decide and exposure confirmation belong to the data plane.
- Local development composes both hosts over one explicitly configured,
  registry-owned file adapter. Writes must be atomic, reads must observe
  cross-process approvals, and storage failure must not fall back to a
  process-local registry.
- Application deployment is external and must not be modeled as a Polari-owned operation.
- MVP trusted application/bootstrap startup acts as the first control-plane client: it atomically applies the statically extracted bundle before enabling data-plane calls.
- Future clients may use startup verify-only, CLI, CI/CD, GitOps, init/deployment hooks, or registry-first workflows without changing service boundaries.
- Browser bundles must not contain management credentials; the local Tetris MVP uses a trusted local bootstrap host or explicitly insecure local-development mode.
- Production decide requires exact registered `definitionId + revision + contractDigest`.
- Contract/configuration errors return Problem Details and never invoke local fallback.
- Governed server fallback remains a `200` audited result for valid definitions blocked by policy, evidence, or state.
- SDK-local fallback is optional and limited to recognized data-plane availability failures.

## Parallel implementation seams

After the API artifacts merge, client and service work can proceed independently.

### Client track

The client track owns:

- typed signal handles and code-first decision authoring,
- fail-closed static extraction,
- canonical definition normalization and digesting,
- generated definition bundle output,
- HTTP serialization from bound inputs/context,
- compact definition identity propagation,
- `flaggo.tune.number(...)` decision receipts,
- exposure confirmation,
- local client fallback for service unavailability,
- telemetry emission.

It tests against a generated mock server or fixture-backed HTTP harness derived from OpenAPI.

### Service track

The service track owns:

- runtime and management endpoints,
- definition registry and contract-integrity verification,
- target resolution,
- inference-input validation,
- evidence provider,
- governed state store,
- deterministic strategy execution,
- policy evaluation,
- audit recording,
- exposure confirmation and attribution linkage,
- local adapters and health reporting.

It tests requests and responses against the same OpenAPI schemas and golden fixtures used by the client.

### Service component ports

The Decision API composes module-owned interfaces:

```text
DecideRequest
  -> IRuntimeDefinitionReader
  -> ITargetResolver
  -> IEvidenceProvider
  -> IStateStore
  -> IStrategyExecutor or fixed active value
  -> IPolicyEvaluator
  -> IAuditSink
  -> RuntimeDecisionResult
```

Port DTOs such as evidence, state, strategy execution, policy evaluation, and audit requests are owned by their component boundaries. Cross-component wire/domain contracts remain in [Shared Contracts](design/shared-contracts/README.md).

## Authority paths

Phase 3 uses bundle-approved initial authority and does not invoke decision
intelligence. Phase 4 adds proposal-managed authority at a high level; both
paths converge on the same `GovernedDecisionState` activation and runtime
execution boundaries described by [Decision Lifecycles](DECISION_LIFECYCLES.md)
and [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md).

## MVP Tetris flow

```text
1. Definition sync
   Register tetris.dropInterval as number, 200-1500ms, step 50, fallback 800,
   with a bundle-approved initial numeric rule.

2. Approval and activation
   An authenticated actor approves the exact bundle snapshot; the service
   derives proposal/activation identities and publishes governed state.

3. Runtime decision
   Game calls POST /v1/decisions/tetris.dropInterval:decide with target/metadata context
   plus bound inference inputs: boardPressure, recentPlacementTimeMs,
   recoveryFailures, and currentLevel.

4. Strategy execution
   Decision API loads active strategy and calculates the immediate value.

5. Governance
   Policy checks min/max, step, max delta, and fallback.

6. Response
   API returns RuntimeDecisionResult with value, decisionId, decision mode, targets,
   policy result, fallback provenance, reason, and auditId.

7. Exposure
   Game applies the value and confirms exposure with decisionId.

8. Feedback
   Client emits outcome telemetry linked to the confirmed exposure. Phase 3
   does not ingest it to generate replacement authority.
```

## MVP design and implementation plan

The MVP should proceed API-contract first. The API artifacts are the blocking dependency; after they merge, client and service implementation should branch from the same contract revision and proceed in parallel.

### Phase 0: Repository and contributor baseline

Goal: make Flaggo easy to run and understand as an open-source project.

Repository decision: Flaggo's open-source core uses a modular monorepo. Logical
components retain explicit module-owned interfaces and may be separate
deployables without becoming separate source repositories. See
[Repository Architecture](REPOSITORY_ARCHITECTURE.md).

Deliverables:

- root README with product framing and quickstart,
- clear repository layout for SDK, server, shared contracts, and demo,
- local development command,
- container-friendly server startup,
- example environment configuration,
- license and contribution expectations when the project is ready to publish.

Decision rule: if a new contributor cannot run the MVP locally without cloud setup, the foundation is not portable enough.

### Phase 1: Executable API contract production

Goal: create executable, language-neutral contracts that let client and service teams implement independently.

Deliverables:

- runtime OpenAPI for decide, exposure confirmation, and health,
- management OpenAPI for definition-bundle validate/apply and semantic-revision approval,
- JSON Schema for `DecisionDefinitionBundle` and Problem Details extensions,
- canonical normalization and digest specification,
- stable error, fallback provenance, target provenance, compact confidence, policy, and contract-integrity shapes,
- exposure-confirmation request/response contract,
- concrete liveness/readiness schemas and dependency-state behavior,
- golden request/response fixtures for success, governed fallback, client fallback, target-claim replacement, validation, approval, authentication, retry, and conflict paths,
- generated or hand-verified TypeScript/server model conformance,
- mock server or fixture harness consumable by the client track.

Validation:

- every OpenAPI example and JSON fixture validates,
- combined code-first and explicit definitions normalize to the same canonical digest,
- invalid inputs, objectives, policies, strategies, and definition identities have stable errors,
- server and client fallback provenance are distinguishable,
- exposure confirmation cannot occur without a valid `decisionId`,
- replayed exposure confirmation returns the original `exposureId`,
- semantic-change apply performs no mutation before explicit approval,
- approved receipts provide a complete per-decision accepted runtime tuple,
- SDK binding lookup selects `acceptedDefinitions[decisionKey]` and propagates `contractDigest`,
- approval terminal transitions, expiry renewal, idempotency, and concurrency fixtures pass,
- approval fixtures expose old/new digests, semantic diff, immutable snapshot, and persisted actor/comment,
- approval snapshot fixtures use quoted entity tags and RFC 9530 SHA-256 content digests,
- optional definition lineage omission/resolution and supplied-ID mismatch rules pass,
- same-key decide retry returns the original decision and conflicting reuse returns `409`,
- concurrent decide retries, TTL expiry, and tracing-field exclusion follow the fingerprint contract,
- availability fallback occurs only for the exact eligible classifier after binding verification and retry exhaustion,
- `required-evidence-unavailable` requires both explicit effective-policy permission and SDK configuration before client fallback,
- global health readiness fails for required dependencies and remains degraded for evidence loss regardless of per-definition evidence requirements,
- full evidence detail remains in audit rather than runtime results,
- direct REST clients can implement the flow without the TypeScript SDK.

Exit gate: **accepted on 2026-07-31.** The complete artifact tree exists, the
listed validations pass, and the executable contracts are the shared baseline
for Phase 2 implementation.

### Phase 2: Parallel client and service implementation

Goal: build both sides concurrently against the same frozen API artifacts.

#### Track 2A: TypeScript client library

Deliverables:

- `createFlaggoClient`,
- typed event, metric, derived-metric, and inference-input handles,
- combined `flaggo.tune.number(...)` authoring/runtime API,
- fail-closed static extraction subset,
- canonical definition/bundle normalization and digesting,
- trusted startup registration and registration-receipt handling,
- typed `requires-approval` startup handling and approval request propagation,
- HTTP client generated from or checked against OpenAPI,
- compact definition identity propagation,
- `DecisionReceipt<number>` and detailed result projection,
- exposure confirmation,
- optional decide idempotency-key support,
- OAuth 2.0/OIDC credential providers with explicit local-development bypass,
- explicitly configured data-plane availability fallback,
- telemetry emission and OpenTelemetry-compatible mode.

Validation:

- SDK contract tests pass against the shared fixture/mock server,
- invalid authoring fails extraction or TypeScript validation,
- repeated calls reuse cached static descriptors,
- same-key conflicting definitions fail closed,
- concurrent startup registration of the same bundle is idempotent,
- failed startup registration rejects initialization; `requires-approval` is a typed startup error, and no data-plane client is created,
- runtime requests send bound values and compact identity, not full definitions,
- client fallback never claims server decision, policy, or audit identity,
- verified/replaced cohort provenance is available in detailed results.

#### Track 2B: Decision service and local adapters

Deliverables:

- runtime decide and exposure-confirmation endpoints,
- definition validate/apply endpoints,
- definition approval status/approve/reject endpoints,
- `IRuntimeDefinitionReader` and `IIntelligenceDefinitionReader` local implementation,
- contract-integrity and canonical digest verification,
- `ITargetResolver`,
- `IStateStore` local implementation,
- `IEvidenceProvider` local implementation,
- `IStrategyExecutor` for fixed and numeric-rule strategies,
- `IPolicyEvaluator` for bounds, step, max delta, cooldown, evidence/model constraints, pause, and fallback,
- `IAuditSink` local implementation,
- exposure record linkage,
- OAuth 2.0/OIDC scope enforcement and explicit local-development bypass,
- decide idempotency storage and conflict detection,
- health endpoint and structured logs.

Validation:

- service contract tests pass against every shared fixture,
- fixed fallback works when no governed state exists,
- active numeric-rule strategy returns bounded adaptive values,
- policy blocks invalid or cooldown-violating candidates,
- invalid inference signals, objectives, policies, and definition identities fail closed,
- every server result contains the required decision/audit/fallback/contract-integrity fields,
- exposure confirmation creates attribution identity only after a decision is applied,
- semantic-change apply leaves registry state unchanged until approval, then applies atomically,
- runtime results expose compact confidence and target provenance while audit retains full evidence.

Parallel-work rule: Track 2A and Track 2B may not change shared wire semantics independently. Any contract change first updates OpenAPI/schema, canonical fixtures, and both conformance suites.

Phase 2 conformance is executable at both implementation boundaries:
`npm run check:openapi` validates the SDK-required operations and schemas
directly from the frozen OpenAPI YAML documents, while the .NET service suite
requires explicit schema, endpoint-owner, authentication, and behavior
assertions for every case in the shared fixture manifest. Management routes are
hosted only by the control-plane executable; runtime decide, exposure, and
health routes are hosted only by the data-plane executable. Evidence and policy
ports and local adapters are owned by their respective modules and injected
into reasoning.

### Phase 2.5: Local SDK-service acceptance demo

Goal: prove the completed SDK and decision service work together in a minimal,
cloud-free application before introducing the Tetris UI or a telemetry-backed
closed loop.

Scope boundary: this phase verifies extraction, registration, decision
consumption, policy behavior, exposure confirmation, telemetry emission, and
fallback semantics. The application computes its inference metric locally.
Telemetry ingestion, server-side metric derivation, evidence updates, and
learning from outcomes remain Phase 4 concerns.

Scenario: implement a TypeScript console application under
`examples/adaptive-worker/` that processes an in-memory work queue. Flaggo
decides `demo.workerBatchSize`, and the worker applies that value as the number
of queue items processed per cycle.

Each work item should contain only the fields needed to produce observable,
deterministic behavior:

```ts
interface WorkItem {
  id: string;
  processingMs: number;
  shouldFail: boolean;
}
```

The workload should use deterministic predefined profiles rather than
uncontrolled randomness:

- steady: enqueue two normal-cost items per tick,
- burst: enqueue eight items per tick,
- slow downstream: enqueue three higher-cost items per tick,
- recovery: enqueue one item per tick.

Seeded randomness may vary item cost or failure within a profile, but the seed
must be configurable and test runs must remain reproducible.

The worker derives queue pressure from actual queue state and supplies it as a
live inference input. The initial formula should remain transparent and
bounded:

```text
queuePressure = clamp(
  0.7 * queueDepth / queueCapacity
  + 0.3 * oldestItemAgeMs / targetLatencyMs,
  0,
  1
)
```

For this phase, `demo.queuePressure` is declared as an app-emitted metric
because the local service does not yet ingest events to calculate derived
metrics. The worker should also emit:

- `demo.itemEnqueued`,
- `demo.itemCompleted`,
- `demo.queueDepth`,
- `demo.queuePressure`,
- `demo.processingLatencyMs`.

Deliverables:

- minimal TypeScript application using the built `@flaggo/sdk`,
- statically extracted canonical bundle for `demo.workerBatchSize`,
- startup registration against the local .NET host,
- development-only service scenario that seeds the registered definition,
  numeric-rule governed state, deterministic evidence snapshot, and
  authoritative target mapping,
- numeric action space from `1` to `10` with step `1` and fallback `3`,
- deterministic rule that returns batch size `3` below pressure `0.7` and `6`
  at or above pressure `0.7`,
- policy bounds, maximum delta `3`, and a short cooldown,
- application of each returned batch size before exposure confirmation,
- telemetry sink that exposes emitted signals locally, preferably through the
  OpenTelemetry-compatible SDK adapter,
- documented commands for starting the service and running the worker,
- automated smoke test covering the complete SDK-to-service path.

Validation:

- the extractor emits a deterministic bundle and descriptor for the worker,
- startup registration returns a receipt whose accepted binding matches the
  extracted contract digest,
- steady load selects and applies batch size `3`,
- burst load selects and applies batch size `6` within policy,
- recovery returns toward batch size `3` without violating cooldown,
- queue pressure is calculated from real queued items rather than generated as
  an independent random input,
- exposure confirmation occurs only after the selected batch size is applied,
- repeated confirmation returns the original exposure identity,
- emitted telemetry contains the expected signal keys, schema digests, values,
  and timestamps,
- service-policy fallback and SDK availability fallback are distinguishable,
- stopping the service uses local fallback `3` only when explicitly enabled,
- the demo runs without cloud services or the Tetris application.

Non-goals:

- feeding emitted telemetry back into service evidence,
- server-side aggregation or derived-metric calculation,
- adapting policy or strategy from observed outcomes,
- durable queue, registry, state, evidence, exposure, or audit storage,
- production authentication or deployment.

### Phase 3: Bundle-approved Tetris hero

Goal: prove the hero scenario end to end without requiring decision
intelligence. Phase 3 uses declarative authority authored in the definition
bundle, explicitly approved by an authenticated control-plane actor, activated
as governed state, and consumed by the existing deterministic runtime path.

Scope boundary:

- the bundle declares an initial authority candidate but cannot approve itself;
- bundle approval authorizes the exact definition snapshot and its initial
  authority;
- registration is not ready until required initial authority is active;
- outcome telemetry is emitted and attributed, but it is not ingested to learn
  or propose a replacement strategy;
- proposal-managed authority remains Phase 4.

The replacement definition-bundle contract separates authority workflow from
runtime execution kind:

```json
{
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
  }
}
```

`initialAuthority` is an approval candidate, not active state. It participates
in semantic identity for the MVP, so changing its target, rule, or rationale
creates a new definition revision and requires a new approval. The server
derives proposal and activation identities; callers cannot supply trusted
approval or state identities.

Deliverables:

- preserve registry-owned runtime and lifecycle projections and
  definition-driven target resolution completed by #30;
- complete #39 to review the state lifecycle introduced by #31 and retain only
  state identity, generation, expected-baseline compare-and-swap, idempotent replay,
  predecessor linkage, approval reference, validation, read-only runtime
  projection, and atomic publication;
- complete #40 to replace the frozen bundle v1 contract rather than adding a
  compatibility layer, and remove the mixed-purpose `onlineStrategy`
  declaration;
- validate the initial numeric rule against the definition target hierarchy,
  inference inputs, action space, fallback, and runtime policy;
- durably record authenticated bundle approval before activating the derived
  state, and keep registration non-ready until activation succeeds;
- expose activated-authority proposal, activation, state, generation, target,
  and kind references in the completed registration receipt;
- complete #41 to migrate `examples/tetris-integration` so the canonical bundle
  owns the numeric rule and control target;
- remove direct bootstrap state publication and static model-style confidence
  evidence from the intended Tetris path;
- preserve the existing `750ms`/`850ms` runtime behavior, `800ms` server and
  client fallback distinction, exposure confirmation, telemetry linkage, and
  audit visibility.

Validation:

- apply validates the complete definition and initial-authority candidate;
- a semantic creation or change cannot initialize the SDK before approval;
- authenticated approval activates the exact bundle-derived authority;
- registration retry after interruption returns the original activation and
  state identities;
- exact bundle replay does not create another state;
- changed initial authority requires a new approval;
- stale expected baseline cannot overwrite newer bundle-approved or
  proposal-managed authority;
- SDK and direct REST calls remain contract-equivalent;
- high pressure and slow placement return `850ms`, while recovery returns
  `750ms`, within bounds and step;
- bundle-authored rules do not claim model confidence or require fabricated
  evidence quality;
- exposure is committed only after application confirmation, and unused
  decisions do not create exposure records.

The current manual activation template and deterministic evidence fixture are
historical evidence for the runtime path only. Phase 3 is complete again only
after the bundle-approved path replaces them and the real-host integration is
rerun. Temporal cooldown and previous-result stabilization semantics remain
owned by #33 and are not silently resolved by this re-baseline.

### Phase 4: Proposal-managed intelligence and governance

Goal: add independently generated authority changes on top of the completed
bundle-approved runtime and activation path. This work is tracked by #25 after
#41 and #33.

Deliverables:

- a scripted or fixture-based proposal producer,
- independent proposal review and approval,
- governed replacement activation through the shared state boundary,
- lifecycle audit and an operator-facing inspection path,
- a documented extension point for future evidence-backed or AI-assisted
  proposal producers.

### Phase 5: Packaging and portable deployment

Goal: make the MVP portable before adding provider-specific integrations.

Deliverables:

- container image or container-ready startup,
- local compose-style deployment if needed,
- published OpenAPI and JSON Schema artifacts from Phase 1,
- OpenTelemetry collector-compatible configuration,
- adapter interface documentation,
- contributor quickstart covering server, SDK, and Tetris,
- one documented path for a future cloud-backed adapter.

Validation:

- local run still works with no cloud services,
- cloud-specific code remains behind adapters,
- API and manifest contracts remain provider-neutral.

## Recommended MVP build sequence

1. Establish the repository/contributor baseline.
2. Design and merge OpenAPI, JSON Schema, canonical fixtures, and contract tests.
3. Branch client and service tracks from the same merged contract revision.
4. Implement SDK and service concurrently against fixture-based conformance suites.
5. Prove SDK-to-service interoperability with the local adaptive-worker acceptance demo.
6. Integrate frequently; do not wait for either track to be feature-complete.
7. Preserve executable definition semantics and reduce governed-state mutation
   to the shared activation core.
8. Implement bundle-approved initial authority and rerun Tetris through
   approval, activation, decide, exposure confirmation, telemetry, and audit.
9. Clarify temporal stabilization semantics.
10. Add proposal-managed generation, review, and replacement activation.
11. Package local startup and publish the contract artifacts and quickstart.
12. Add optional cloud adapters only after the local MVP is stable.

## Branch and integration discipline

- The API-contract branch merges before client/service implementation branches are created.
- Client and service branches record the contract artifact revision they implement.
- Contract-breaking changes require a dedicated contract PR that updates OpenAPI/schema, fixtures, compatibility notes, and both conformance suites.
- Client and service tracks should merge small vertical increments and run cross-track contract tests continuously.
- Phase 3 integration begins as soon as both tracks can complete one fixture-backed decide call; it is not deferred until all Track 2 deliverables finish.

## Extensibility checkpoints

Before adding features, verify the change extends one of these seams instead of bypassing it:

- new result shape: should not be added unless boolean/number/string is insufficient,
- new strategy type: extend `DecisionStrategy`,
- new evidence source: implement `IEvidenceProvider`,
- new storage backend: implement the registry read/lifecycle ports or `IStateStore`,
- new policy rule: extend `IPolicyEvaluator`,
- new async reasoning mode: implement `IDecisionIntelligence`,
- new telemetry transport: extend SDK telemetry mode without changing decision calls,
- new cloud provider: implement adapters behind existing ports instead of changing core contracts.
