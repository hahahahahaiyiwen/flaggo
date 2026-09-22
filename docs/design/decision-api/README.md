# Decision API Design

## Purpose

The Decision API is the runtime service applications call when they need a `RuntimeDecisionResult` from flaggo.

It receives a decision key, runtime target/context, application identity, and
optional request metadata. It resolves the applicable decision definition,
control target, governed state, policy, and evidence only when required,
records audit context, and returns a value or fallback guidance.

The runtime API should also verify compact definition identity when the client or deployment provides it. A decision must not be returned as approved when the caller's definition ID or revision is unknown, retired, or semantically conflicting.

## Design goals

- Provide a small runtime API for application decision calls.
- Be safe-by-default: return fallback when applicable evidence, policy, or
  service state is insufficient.
- Keep decision responses explainable and auditable.
- Separate application execution from decision evaluation.
- Support scope resolution.
- Support deterministic policy gating before any candidate action is returned as approved.
- Avoid assuming every decision requires an LLM.
- Execute approved strategies quickly for real-time adaptive decisions.

MVP implementation guidance: [MVP Implementation Guide](../../IMPLEMENTATION_GUIDE.md).
Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Phase 1 wire-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md).
Runtime execution model: [Runtime Decision Execution](../../RUNTIME_DECISION_EXECUTION.md).

## Runtime responsibility

At a high level:

```text
request(decision key, runtime context, signal inputs)
  -> validate decision key and definition identity
  -> reject duplicate input keys
  -> verify every input resolves to an allowed app-emitted primitive metric
  -> verify metric objectives resolve to numeric metrics and obey direction/target invariants
  -> verify policy is present
  -> verify expected contract digest/revision when supplied
  -> load decision definition
  -> resolve the definition-owned target chain
  -> fetch governed state
  -> fetch telemetry evidence and assess uncertainty only when required
  -> load active fixed value, strategy, experiment, rollout, override, or fallback
  -> execute the matching approved runtime mechanism
  -> apply deterministic runtime policy checks
  -> record audit/explanation
  -> return RuntimeDecisionResult, possibly containing fallback
```

## Initial API shape

Accepted Phase 1 shape:

```http
POST /v1/decisions/{decisionKey}:decide
```

Flaggo-owned REST APIs should use path-based versioning:

```text
/v1/...
```

Version query parameters should be avoided for Flaggo APIs. Query parameters should be reserved for filtering, pagination, and optional read behavior.

OpenTelemetry ingestion should follow standard OTLP conventions where possible rather than inventing Flaggo-specific versioning around telemetry transport.

## API groups

Flaggo should separate runtime APIs from management APIs.

The required Phase 1 executable artifact set covers decide, exposure confirmation, definition-bundle validate/apply/approval, and health. The broader resource groups below describe future component boundaries; they are outside this artifact set.

Runtime decide and exposure confirmation form the data plane. Definition-bundle validate/apply and lifecycle operations form the control plane. Application deployment is external to both: it may happen without control-plane publication, but data-plane calls succeed only for exact registered identities.

### Runtime APIs

Runtime APIs are called by application code while the application is running.

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

Primary purpose:

- evaluate a scoped decision,
- return approved value or fallback,
- emit audit correlation,
- confirm application/rendering separately so a returned decision is not mistaken for an exposure.

### Telemetry ingestion APIs

Telemetry ingestion should prefer OpenTelemetry Protocol (OTLP) compatibility.

Possible endpoints depend on the OTLP transport selected:

```text
/v1/traces
/v1/metrics
/v1/logs
```

or Flaggo may expose these behind a dedicated ingest host/path if needed:

```text
/otel/v1/traces
/otel/v1/metrics
/otel/v1/logs
```

Design rule:

> Use OpenTelemetry standards for telemetry transport; use Flaggo APIs for decision semantics.

Flaggo-specific telemetry management can come later for app registration, source validation, sampling policy, retention, and expected event/metric declarations.

### Management APIs

Management APIs configure what Flaggo knows and governs.

Initial resource groups:

```http
/v1/apps
/v1/decisions
/v1/contracts
/v1/policies
/v1/strategies
```

Responsibilities:

- register applications and environments,
- register decision keys and definitions,
- manage decision contracts,
- define result type and action space,
- define target hierarchy,
- define evidence requirements,
- define goals and fallback contracts,
- register policy constraints,
- approve bundle-declared initial authority and activate derived
  `GovernedDecisionState`,
- govern later strategies produced by async intelligence or operator tooling
  through the same activation boundary.

Decision resources should be managed as versioned, append-only contracts with a simplified lifecycle:

```text
active -> deprecated -> retired
```

Management APIs should support contract-bundle sync so build/deploy tooling can
register or validate resources owned by a codebase. Missing resources should
not be hard-deleted automatically; they should become deprecation candidates
and require explicit lifecycle transition.

### State and operator APIs

State/operator APIs control live decision behavior.

Initial resource groups:

```http
/v1/state
/v1/overrides
/v1/rollbacks
```

Responsibilities:

- inspect active values,
- inspect active strategies,
- pause or resume decisions,
- apply operator overrides,
- clear overrides,
- trigger rollback,
- inspect cooldown or rollout state.

### Audit and query APIs

Audit/query APIs expose decision history and explanation records.

Initial resource groups:

```http
/v1/audit
/v1/decisions
```

Example read query:

```http
GET /v1/audit?decisionKey=tetris.dropInterval&scope=user:user-123
```

Responsibilities:

- list decision history,
- inspect individual audit records,
- query by decision key, definition, runtime target, control target, evidence view, time window, policy result, or fallback status,
- support operator console views.

Example:

```json
{
  "runtimeTarget": {
    "type": "session",
    "id": "game-456"
  },
  "runtimeContext": {
    "userId": "user-123",
    "sessionId": "game-456",
    "cohort": "new_players",
    "deviceType": "mobile"
  },
  "inputs": [
    { "signal": { "key": "tetris.boardPressure" }, "value": 0.82 },
    { "signal": { "key": "tetris.currentLevel" }, "value": 3 },
    { "signal": { "key": "tetris.recentPlacementTimeMs" }, "value": 1300 },
    { "signal": { "key": "tetris.recoveryFailures" }, "value": 2 }
  ],
  "expectedContract": {
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "contractDigest": "sha256:contract...",
    "bundleDigest": "sha256:bundle...",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
    "buildId": "tetris-web-2026-07-25.1",
    "deploymentId": "tetris-web-2026-07-25.1"
  },
  "client": {
    "appId": "tetris-demo",
    "environment": "dev",
    "sdk": "typescript",
    "sdkVersion": "0.1.0"
  }
}
```

For the HTTP route `POST /v1/decisions/{decisionKey}:decide`, the path supplies the stable decision key and the wire body does not duplicate it. The service normalizes the route parameter and body into the shared internal `DecideRequest`, including the decision key, expected definition identity, runtime target, context, and inputs.

Response:

```json
{
  "decisionKey": "tetris.dropInterval",
  "definition": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "decisionId": "decision-789",
  "runtimeTarget": {
    "type": "session",
    "id": "game-456"
  },
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "targetProvenance": [
    {
      "targetType": "cohort",
      "claimedId": "new_players",
      "resolvedId": "new_players",
      "source": "client-verified"
    }
  ],
  "resolutionChain": [
    "session:game-456",
    "cohort:new_players",
    "global"
  ],
  "value": 850,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
  "confidence": null,
  "fallback": {
    "source": "server",
    "resolutionFallbackUsed": true,
    "decisionFallbackUsed": false,
    "reason": "no_active_session_authority"
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "step", "max-delta"]
  },
  "definitionStatus": {
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
    "contractDigest": "sha256:contract...",
    "bundleDigest": "sha256:bundle...",
    "buildId": "tetris-web-2026-07-25.1",
    "deploymentId": "tetris-web-2026-07-25.1",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "exposure": {
    "confirmationRequired": true,
    "confirmToken": "confirm-789"
  },
  "reason": "The approved weighted numeric rule met its 0.55 threshold.",
  "auditId": "audit-789"
}
```

Resolution fallback response:

This means Flaggo could not use the most specific requested scope, but it still made an approved decision from a broader scope in the resolution chain.

```json
{
  "decisionKey": "tetris.dropInterval",
  "definition": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "decisionId": "decision-791",
  "runtimeTarget": {
    "type": "session",
    "id": "game-456"
  },
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "targetProvenance": [
    {
      "targetType": "cohort",
      "claimedId": "new_players",
      "resolvedId": "new_players",
      "source": "client-verified"
    }
  ],
  "resolutionChain": [
    "session:game-456",
    "cohort:new_players",
    "global"
  ],
  "value": 750,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
  "confidence": null,
  "fallback": {
    "source": "server",
    "resolutionFallbackUsed": true,
    "decisionFallbackUsed": false,
    "reason": "no_active_session_authority"
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "step", "max-delta"]
  },
  "definitionStatus": {
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
    "contractDigest": "sha256:contract...",
    "bundleDigest": "sha256:bundle...",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "exposure": {
    "confirmationRequired": true,
    "confirmToken": "confirm-791"
  },
  "reason": "The request resolved to cohort authority and the weighted score was below 0.55.",
  "auditId": "audit-791"
}
```

Decision fallback response:

This means Flaggo could not safely make an approved decision at any applicable scope and returned the configured fallback value.

```json
{
  "decisionKey": "tetris.dropInterval",
  "definition": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "decisionId": "decision-790",
  "runtimeTarget": {
    "type": "session",
    "id": "game-456"
  },
  "targetProvenance": [],
  "resolutionChain": [
    "session:game-456",
    "cohort:new_players",
    "global"
  ],
  "value": 800,
  "valueType": "number",
  "decisionMode": "fallback",
  "confidence": null,
  "fallback": {
    "source": "server",
    "resolutionFallbackUsed": false,
    "decisionFallbackUsed": true,
    "reason": "missing_state"
  },
  "policy": {
    "result": "fallback",
    "reasons": ["missing_state"],
    "appliedConstraints": []
  },
  "definitionStatus": {
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
    "contractDigest": "sha256:contract...",
    "bundleDigest": "sha256:bundle...",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "exposure": {
    "confirmationRequired": false
  },
  "reason": "No permitted target had compatible active authority; returned the registered fallback.",
  "auditId": "audit-790"
}
```

This fallback selected no authority. It therefore omits `controlTarget`,
`strategyId`, state lineage, and exposure confirmation. The ordered
`resolutionChain` records which permitted targets were attempted; it does not
fabricate a global authority or evidence claim.

## Request responsibilities

The request should provide:

- decision key and definition,
- requested scope when the client knows it,
- runtime context,
- required expected definition ID, revision, and contract digest,
- client/app metadata,
- optional correlation IDs.

The request should not provide:

- arbitrary model prompts,
- unbounded action candidates,
- policy bypass flags,
- raw implementation-specific state.

## Response responsibilities

The response should provide:

- selected value,
- value type,
- decision mode,
- strategy ID when an approved strategy produced the value,
- fallback status separated into resolution fallback and decision fallback,
- resolved control scope when active authority produced the result,
- verified target provenance,
- policy result,
- contract integrity status,
- compact confidence status,
- human-readable reason,
- audit correlation ID.

The response should not expose:

- full internal model reasoning,
- sensitive raw telemetry,
- unrelated policy configuration,
- implementation-specific storage details.

## Scope resolution

The Decision API should resolve target roles using:

- requested scope,
- runtime context,
- target hierarchy,
- configured fallback chain.

For Tetris:

```text
runtime target: session:game-456
resolution chain: session:game-456 -> cohort:new_players -> global
```

The runtime target and resolution chain should be included in the response.
The control target and its provenance are present only when an active authority
was selected. Full evidence-view detail belongs in the audit record rather than
the latency-sensitive runtime response.

Client-supplied cohort or segment IDs are claims, not authority. The service verifies the claim or replaces it using trusted server-side attributes and records `client-verified`, `server-derived`, or `server-replaced` provenance in the result.

Resolution fallback should not be treated as a failed decision. If Flaggo
resolves from a requested `session` target to approved `cohort` authority, the
response is still approved. Confidence is present only when that authority
makes an evidence-backed claim.

## Policy evaluation

The Decision API must not return a candidate as approved until policy passes.

Policy may block or force fallback because of:

- out-of-range value,
- max delta violation,
- no compatible active state at any permitted target,
- applicable evidence, uncertainty, guardrail, temporal, or operator
  constraints.

Policy reason codes should be stable because clients, audits, and the operator console may depend on them.

Missing, duplicate, invalid, or nonfinite required inference inputs and invalid
runtime context are rejected during request validation with Problem Details;
they do not enter policy evaluation or become fallback. Contract identity and
registration-readiness failures likewise remain contract errors.

A pending activation, failed readiness check, corrupt/torn persisted state, or
state that violates `DecisionState` invariants is a service/readiness error.
Those failures must not be converted into `missing_state` fallback.

## Contract integrity

The Decision API should compare the request's expected contract identity with
registry state before approving a decision. It must accept concurrent rolling-
deployment requests for different known identities without substituting
revisions. Because one stable authority head serializes each decision key and
control target, an older accepted identity receives server fallback when that
head no longer contains compatible state.

Success and error states:

| State | Runtime behavior |
| --- | --- |
| `verified` | The full expected definition ID/revision/digest tuple is registered and lifecycle-eligible; return a server `200` containing the same tuple. |
| `missing-contract-identity` | Required tuple member is missing; return `400` Problem Details. |
| `contract-not-registered` | Definition ID/revision is unknown; return `409` Problem Details. |
| `contract-conflict` | The digest does not match the registered revision; return `409` Problem Details. |
| `unknown-decision-key` | Decision key is not registered; return `404` Problem Details. |
| `retired-definition` | The exact revision is retired; return `409` Problem Details. |

The full contract bundle should not be sent on each runtime request.

Contract/configuration failures are not decision fallback. The service does not execute an older revision, and the SDK must not convert the 4xx response into local fallback.

## Authentication and retry identity

Production data-plane requests use OAuth 2.0/OIDC access tokens. Decide requires `polari.decisions:decide`; exposure confirmation requires `polari.exposures:confirm`. Authorization also verifies the application and environment carried by the registered contract identity. Local development may enable an explicit insecure bypass, which must be disabled by default and visibly diagnosed.

`POST /v1/decisions/{decisionKey}:decide` accepts an optional `Idempotency-Key` header:

- the namespace, RFC 8785 fingerprint, 24-hour retention, and concurrent-request behavior are defined by the [API Contract Proposal](../API_CONTRACT_PROPOSAL.md#correlation-and-retries),
- the same key and fingerprint returns the original decision result,
- reuse with another fingerprint returns `409 idempotency-conflict`,
- omitting the header creates a new decision record,
- `X-Flaggo-Correlation-Id` and trace metadata are excluded from the fingerprint;
  the adapter maps the resolved header into internal `correlationId`.

Phase 1 exposes only the singular decide operation. Batch decisions are deferred until ordering, partial-failure, policy, and idempotency semantics can be designed explicitly.

## Strategy execution

For real-time adaptive decisions, the Decision API should execute an active governed strategy rather than run deep analysis in the online request path.

MVP strategy execution:

```text
active strategy
  -> evaluate the declared rule against request inputs
  -> calculate candidate value
  -> validate action space and strategy contract
  -> check applicable runtime policy
  -> pass candidate to policy
  -> return approved value or fallback
```

The first strategy executor can support only numeric rule strategies for `tetris.dropInterval`. Future executors can add fixed value, scoring, bandit, model, or experiment strategies behind the same interface.

Intent-level service port:

```ts
interface IStrategyExecutor {
  execute(input: StrategyExecutionRequest): Promise<StrategyExecutionResult>;
}
```

The Decision API should treat strategy execution as a bounded operation. It should not call an unbounded agent loop in the normal online path unless a specific decision definition is explicitly configured for that behavior.

## Fallback and confidence semantics

The API should distinguish two fallback types:

1. **Resolution fallback**
   - Flaggo could not use the requested or most-specific runtime target,
     authority, or required evidence view.
   - Flaggo resolved to a broader target, such as `cohort` or `global`.
   - A real decision may still be approved.
   - Confidence is present only when the approved broader-target authority
     makes an evidence-backed claim.

2. **Decision fallback**
   - Flaggo could not safely approve a decision.
   - The returned value is the configured fallback.
   - Confidence is `null` because no evidence-backed decision was approved.

Confidence is not one generic score. It is required only when a result claims
evidence-backed adaptation. It is `null` for decision fallback,
non-evidence-based active values, and deterministic bundle-authored strategies.
When present, it describes the returned decision at the evidence and control
target used, not necessarily the originally requested runtime target:

| Field | Meaning |
| --- | --- |
| `evidenceQuality` | Freshness, sample size, missingness, and consistency of evidence. |
| `modelUncertainty` | Uncertainty in a learned estimate or strategy; representation must define whether higher or lower is better. |
| `expectedOutcome` | Estimated likelihood or magnitude of satisfying the declared objective. |
Policy eligibility belongs to the `policy` result, not `confidence`, so the two cannot contradict each other.

Fallback provenance must be explicit:

| Source | Meaning |
| --- | --- |
| `server` | Decision API returned an audited `RuntimeDecisionResult`, possibly using policy fallback. Server `decisionId`, `auditId`, `policy`, and definition status are present. |
| `client-fallback` | SDK returned the local default because an explicitly configured data-plane availability fallback was triggered. No server `decisionId`, `auditId`, `policy`, or exposure identity may be claimed. It is forbidden for contract/configuration errors. |

## Exposure confirmation

Returning a value creates a decision record, not an exposure. An approved
authority result includes a confirm token or decision handle; a server fallback
returns `confirmationRequired: false`:

This distinction matters because an application can request a decision without using it. The game may end, the relevant component may unmount, local state may change, or a newer decision may supersede the response before the value is applied. Treating every returned value as an exposure would associate outcomes with behavior the user never experienced and bias later evidence, evaluation, and optimization.

Exposure confirmation closes that gap:

```text
decision returned -> value applied or rendered -> exposure confirmed -> outcomes attributed
```

```json
{
  "decisionId": "decision-123",
  "exposure": {
    "confirmationRequired": true,
    "confirmToken": "confirm-abc"
  }
}
```

The SDK should call the Phase 1 `POST /v1/exposures/{decisionId}:confirm` operation only after the application applies or renders the value. Accepted decision A4 requires the confirm token and makes replay return the original `exposureId`. The initial runtime response never includes an `exposureId`.

Later outcome telemetry should correlate to the confirmed `exposureId`, not merely the returned `decisionId`. A decision that is never applied remains auditable but must not enter treatment-effect or optimization evidence as if it were observed by the user.

## Audit and correlation

Every decision response should have an audit ID.

The audit record should correlate:

- request metadata,
- decision key,
- decision definition,
- runtime target,
- control target,
- runtime context summary,
- evidence snapshot/view summary when evidence participated,
- governed state summary,
- active value, strategy, or candidate action,
- policy result,
- returned value,
- fallback usage,
- reason text.

## First slice

For the Tetris hero scenario, the first Decision API should support:

- `POST /v1/decisions/{decisionKey}:decide`,
- `POST /v1/exposures/{decisionId}:confirm`,
- number decisions,
- session/user/cohort/global target resolution,
- resolution fallback and decision fallback response fields,
- nullable confidence and policy result fields,
- audit ID generation,
- an optional evidence snapshot seam,
- deterministic policy evaluation,
- active numeric rule strategy execution.

## Accepted Phase 1 contract decisions

The complete rationale is tracked in the [API Contract Proposal decision log](../API_CONTRACT_PROPOSAL.md#contract-decision-log). The Decision API follows these accepted decisions:

- governed fallback is a completed audited `200`; malformed and configuration failures use Problem Details (A3),
- exposure confirmation requires an opaque token and is idempotent (A4),
- client cohort/segment claims are verified or replaced server-side (A8),
- runtime responses contain compact confidence when applicable and target
  provenance while full evidence remains in audit (A9),
- batch decisions are deferred (A10),
- production authentication uses OAuth 2.0/OIDC scopes with explicit local-development bypass only (A11),
- optional `Idempotency-Key` provides decide retry identity (A12).

Submitting undeclared local evidence is not proposed for v1. The request may carry only typed values for registered inference-input signal keys.
