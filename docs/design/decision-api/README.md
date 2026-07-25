# Decision API Design

## Purpose

The Decision API is the runtime service applications call when they need a governed decision from flaggo.

It receives a decision surface, scope/runtime context, and optional request metadata. It resolves the applicable decision contract and factors, evaluates evidence and policy, records audit context, and returns a value or fallback guidance.

The runtime API should also verify compact contract identity when the client or deployment provides it. A decision must not be returned as approved when the active registered contract is incompatible with what the workload was built to handle.

## Design goals

- Provide a small runtime API for application decision calls.
- Be safe-by-default: return fallback when evidence, policy, or service state is insufficient.
- Keep decision responses explainable and auditable.
- Separate application execution from decision evaluation.
- Support scope resolution.
- Support deterministic policy gating before any candidate action is returned as approved.
- Avoid assuming every decision requires an LLM.
- Execute approved strategies quickly for real-time adaptive decisions.

MVP implementation guidance: [MVP Implementation Guide](../../IMPLEMENTATION_GUIDE.md).
Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## Runtime responsibility

At a high level:

```text
request(surface, runtime context, optional requested scope)
  -> validate surface
  -> verify expected contract digest/revision when supplied
  -> resolve scope chain
  -> load decision contract
  -> fetch telemetry evidence
  -> fetch system state
  -> assess uncertainty
  -> load active governed value, strategy, experiment, override, or fallback
  -> execute active strategy when present
  -> apply governance stage
  -> record audit/explanation
  -> return decision or fallback
```

## Initial API shape

Intent-level shape, not final wire contract:

```http
POST /v1/decisions/{surface}:decide
```

Flaggo-owned REST APIs should use path-based versioning:

```text
/v1/...
```

Version query parameters should be avoided for Flaggo APIs. Query parameters should be reserved for filtering, pagination, and optional read behavior.

OpenTelemetry ingestion should follow standard OTLP conventions where possible rather than inventing Flaggo-specific versioning around telemetry transport.

## API groups

Flaggo should separate runtime APIs from management APIs.

### Runtime APIs

Runtime APIs are called by application code while the application is running.

```http
POST /v1/decisions/{surface}:decide
```

Primary purpose:

- evaluate a scoped decision,
- return approved value or fallback,
- emit audit correlation.

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
/v1/surfaces
/v1/contracts
/v1/policies
/v1/strategies
```

Responsibilities:

- register applications and environments,
- register decision surfaces,
- manage decision contracts,
- define result type and action space,
- define scope hierarchy,
- define evidence requirements,
- define goals and fallback contracts,
- register policy constraints,
- register governed decision strategies produced by async intelligence or operator tooling.

Decision resources should be managed as versioned, append-only contracts with a simplified lifecycle:

```text
active -> deprecated -> retired
```

Management APIs should support manifest-driven sync so build/deploy tooling can register or validate resources owned by a codebase. Missing resources should not be hard-deleted automatically; they should become deprecation candidates and require explicit lifecycle transition.

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
GET /v1/audit?surface=tetris.dropInterval&scope=user:user-123
```

Responsibilities:

- list decision history,
- inspect individual audit records,
- query by surface, scope, time window, policy result, or fallback status,
- support operator console views.

Example:

```json
{
  "surface": "tetris.dropInterval",
  "requestedScope": {
    "type": "session",
    "id": "game-456"
  },
  "runtimeContext": {
    "userId": "user-123",
    "sessionId": "game-456",
    "currentLevel": 3,
    "deviceType": "mobile",
    "boardPressure": "high",
    "recentPlacementTimeMs": 1300,
    "recoveryFailures": 2
  },
  "expectedContract": {
    "digest": "sha256:8fc...",
    "revision": "42",
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

For the HTTP route `POST /v1/decisions/{surface}:decide`, the path supplies the surface. The service should normalize the route parameter and body into the shared `DecideRequest` shape used internally by the Decision API core.

Response:

```json
{
  "surface": "tetris.dropInterval",
  "requestedScope": {
    "type": "session",
    "id": "game-456"
  },
  "resolvedScope": {
    "type": "session",
    "id": "game-456"
  },
  "resolutionChain": [
    "session:game-456",
    "user:user-123",
    "segment:new_players",
    "global"
  ],
  "value": 700,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy-tetris-new-players-v1",
  "confidence": 0.72,
  "evidenceScope": {
    "type": "session",
    "id": "game-456"
  },
  "fallback": {
    "resolutionFallbackUsed": false,
    "decisionFallbackUsed": false,
    "reason": null
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "max-delta", "cooldown"]
  },
  "contract": {
    "revision": "42",
    "digest": "sha256:8fc...",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "reason": "Approved strategy slowed the drop interval because board pressure was high and recent placement time was slow.",
  "auditId": "audit-789"
}
```

Resolution fallback response:

This means Flaggo could not use the most specific requested scope, but it still made an approved decision from a broader scope in the resolution chain.

```json
{
  "surface": "tetris.dropInterval",
  "requestedScope": {
    "type": "user",
    "id": "user-123"
  },
  "resolvedScope": {
    "type": "segment",
    "id": "new_players"
  },
  "resolutionChain": [
    "user:user-123",
    "segment:new_players",
    "global"
  ],
  "value": 750,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy-tetris-new-players-v1",
  "confidence": 0.78,
  "evidenceScope": {
    "type": "segment",
    "id": "new_players"
  },
  "fallback": {
    "resolutionFallbackUsed": true,
    "decisionFallbackUsed": false,
    "reason": "user_scope_insufficient_evidence"
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "max-delta", "cooldown"]
  },
  "contract": {
    "revision": "42",
    "digest": "sha256:8fc...",
    "integrity": "compatible-drift",
    "compatibility": "backward-compatible"
  },
  "reason": "User-level evidence was insufficient; segment-level evidence for new_players supported the returned drop interval.",
  "auditId": "audit-791"
}
```

Decision fallback response:

This means Flaggo could not safely make an approved decision at any applicable scope and returned the configured fallback value.

```json
{
  "surface": "tetris.dropInterval",
  "requestedScope": {
    "type": "user",
    "id": "user-123"
  },
  "resolvedScope": {
    "type": "global",
    "id": "global"
  },
  "resolutionChain": [
    "user:user-123",
    "segment:new_players",
    "global"
  ],
  "evidenceScope": {
    "type": "global",
    "id": "global"
  },
  "value": 800,
  "valueType": "number",
  "decisionMode": "fallback",
  "confidence": null,
  "fallback": {
    "resolutionFallbackUsed": true,
    "decisionFallbackUsed": true,
    "reason": "insufficient_evidence_all_scopes"
  },
  "policy": {
    "result": "fallback",
    "reasons": ["insufficient_evidence_all_scopes"],
    "appliedConstraints": ["min-confidence", "min-sample-size"]
  },
  "contract": {
    "revision": "42",
    "digest": "sha256:8fc...",
    "integrity": "unknown-client-contract"
  },
  "reason": "No scope in the resolution chain had sufficient evidence for a safe decision.",
  "auditId": "audit-790"
}
```

## Request responsibilities

The request should provide:

- decision surface,
- requested scope when the client knows it,
- runtime context,
- expected contract digest/revision when available,
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
- resolved scope,
- evidence scope,
- policy result,
- contract integrity status,
- confidence/evidence status,
- human-readable reason,
- audit correlation ID.

The response should not expose:

- full internal model reasoning,
- sensitive raw telemetry,
- unrelated policy configuration,
- implementation-specific storage details.

## Scope resolution

The Decision API should resolve the decision scope using:

- requested scope,
- runtime context,
- surface-defined scope hierarchy,
- configured fallback chain.

For Tetris:

```text
requested scope: session:game-456
resolution chain: session:game-456 -> user:user-123 -> segment:new_players -> global
```

The resolved scope and resolution chain should be included in the response for auditability.

Resolution fallback should not be treated as a failed decision. If Flaggo falls back from `user` to `segment` and returns an approved segment-level value, the response should still be an approved decision with a confidence score for the resolved evidence scope.

## Policy evaluation

The Decision API must not return a candidate as approved until policy passes.

Policy may block or force fallback because of:

- out-of-range value,
- max delta violation,
- cooldown,
- insufficient sample size,
- low confidence,
- guardrail breach,
- paused operator mode,
- missing required evidence.

Policy reason codes should be stable because clients, audits, and the operator console may depend on them.

## Contract integrity

The Decision API should compare the request's expected contract identity with registry state before approving a decision.

Initial integrity states:

| State | Runtime behavior |
| --- | --- |
| `verified` | Expected digest/revision matches registered compatible contract; decide normally. |
| `compatible-drift` | Expected contract is older but compatible; decide normally and emit diagnostics. |
| `unknown-client-contract` | No known expected identity; allow in local/dev, fallback in production enforce mode. |
| `incompatible-drift` | Expected identity is incompatible with active contract; return fallback and audit. |
| `unknown-surface` | Surface is not registered; return fallback or controlled error. |
| `retired-surface` | Surface is retired; return fallback or controlled error. |

The full contract bundle should not be sent on each runtime request.

## Strategy execution

For real-time adaptive decisions, the Decision API should execute an active governed strategy rather than run deep analysis in the online request path.

MVP strategy execution:

```text
active strategy
  -> evaluate runtime conditions against request context and evidence snapshot
  -> calculate candidate value
  -> clamp to action space and strategy bounds
  -> check max delta and cooldown
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

The Decision API should treat strategy execution as a bounded operation. It should not call an unbounded agent loop in the normal online path unless a specific surface is explicitly configured for that behavior.

## Fallback and confidence semantics

The API should distinguish two fallback types:

1. **Resolution fallback**
   - Flaggo could not use the requested or most-specific scope.
   - Flaggo resolved to a broader scope, such as `segment` or `global`.
   - A real decision may still be approved.
   - Confidence should be present when the broader-scope decision is approved.

2. **Decision fallback**
   - Flaggo could not safely approve a decision.
   - The returned value is the configured fallback.
   - Confidence should be `null` or omitted because no evidence-backed decision was approved.

Confidence describes the returned decision at the `evidenceScope`, not necessarily the originally requested scope.

## Audit and correlation

Every decision response should have an audit ID.

The audit record should correlate:

- request metadata,
- surface,
- resolved scope,
- runtime context summary,
- evidence snapshot,
- system state summary,
- active value, strategy, or candidate action,
- policy result,
- returned value,
- fallback usage,
- reason text.

## First slice

For the Tetris hero scenario, the first Decision API should support:

- `POST /v1/decisions/{surface}:decide`,
- number decisions,
- session/user/segment/global scope resolution,
- resolution fallback and decision fallback response fields,
- confidence and policy result fields,
- audit ID generation,
- simple evidence snapshot integration,
- deterministic policy evaluation,
- active numeric rule strategy execution.

## Open design questions

- Should the API support batch decisions?
- Should the client be allowed to submit local evidence with the decision request?
- Should policy failures return HTTP 200 with fallback, or non-2xx errors?
- How much evidence detail should be included in the runtime response versus audit record only?
- How should segment resolution happen: client-provided, server-derived, or both?
