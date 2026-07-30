# Phase 1 API Contract Proposal

Status: **Decision log accepted; executable contract artifacts pending**

This proposal defines the contract baseline that must be resolved before runtime OpenAPI, management OpenAPI, bundle JSON Schema, golden fixtures, and client/service conformance suites are frozen.

It does not introduce a second domain model. Canonical domain types remain owned by [Shared Contracts](shared-contracts/README.md); this document defines how those types cross HTTP and build/release boundaries.

## Goals

- Let TypeScript client and Decision API teams implement in parallel from one contract revision.
- Keep the online decision call compact, deterministic, typed, and fail-closed.
- Distinguish a governed server fallback from an SDK-local availability fallback.
- Create exposure identity only after the application confirms that it applied a decision.
- Keep contract registration out of the data-plane decision path, even when trusted application/bootstrap startup acts as a control-plane client.
- Keep application deployment independent from control-plane publication without weakening data-plane identity checks.
- Produce executable artifacts and fixtures rather than relying on prose agreement.

## Phase 1 scope

| Contract surface | Phase 1 |
| --- | --- |
| Runtime decision | Required |
| Exposure confirmation | Required |
| Definition-bundle validation | Required |
| Definition-bundle application | Required |
| Liveness/readiness | Required |
| OTLP telemetry ingestion | Reuse the OTLP standard; no Flaggo-specific contract |
| Audit queries | Deferred |
| State, override, rollout, and rollback APIs | Deferred |
| Strategy/proposal management | Deferred |
| Semantic-revision approval | Required |
| Batch decisions | Deferred beyond v1 |

## Control plane, data plane, and application deployment

Polari uses a cloud-service boundary:

| Boundary | Phase 1 behavior |
| --- | --- |
| Control plane | Definition-bundle validate/apply and immutable registry lifecycle. |
| Data plane | Decide and exposure confirmation for exact registered identities. |
| Application deployment | Developer-owned and independent from Polari. |

For MVP, trusted application/bootstrap startup uses the control-plane API to atomically register the statically extracted bundle before enabling data-plane calls. Future clients may publish manually or integrate SDK/CLI tooling into CI/CD, GitOps, release pipelines, init/deployment hooks, verify-only startup, or registry-first workflows. Polari does not claim to block external code deployment.

Code deployed without a registered binding can still run. Startup registration may establish that binding; if it is skipped or fails, decision calls remain disabled. The data plane never registers from decide traffic, silently selects an older revision, or converts a contract/configuration error into local fallback.

Detailed developer UX: [Control Plane and Data Plane UX](client-library/CONTROL_DATA_PLANE_UX.md).

## Sources of truth

| Concern | Authority |
| --- | --- |
| Domain types and invariants | [Shared Contracts](shared-contracts/README.md) |
| HTTP behavior and orchestration | [Decision API](decision-api/README.md) |
| SDK authoring and result projection | [Client Library](client-library/README.md) |
| Bundle lifecycle and compatibility | [Contract Registry](contract-registry/README.md) |
| Exposure and attribution semantics | [Decision Evidence](../DECISION_EVIDENCE.md) |
| This phase's endpoint and artifact boundary | This proposal, until replaced by versioned OpenAPI/JSON Schema |

When prose and an executable artifact disagree after the freeze, the versioned OpenAPI or JSON Schema artifact wins and the prose must be corrected in the same change.

## Contract artifacts

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

Each fixture has:

- a stable scenario name,
- request method, path, headers, and body,
- expected status, headers, and body,
- the canonical definition or bundle digest where relevant,
- a statement of the invariant being tested.

The SDK and service must consume the same fixtures. Generated language types are projections of these artifacts, not independent sources of truth.

## API conventions

### Naming

- Use `decisionKey` consistently in contracts and prose.
- The runtime route carries the decision key; the request body does not duplicate it.
- `surface` is retired as a synonym in new API artifacts.
- JSON fields use `camelCase`.
- Enum wire values use `kebab-case`.
- IDs are opaque strings. Clients must not parse meaning from them.
- Timestamps use RFC 3339 UTC strings.
- SDK duration shorthand such as `"20s"` is normalized into canonical domain fields such as `{ "seconds": 20 }` before transport or hashing.

### Media types

- Success payloads use `application/json`.
- Errors use `application/problem+json` following RFC 9457 Problem Details.
- Unknown JSON fields are rejected on management write APIs and ignored only where the OpenAPI contract explicitly permits forward-compatible extension.

### Correlation and retries

- Clients may send `correlationId`; the service echoes or records it for tracing.
- The service returns its own request correlation header on every response.
- Decide callers may send an optional `Idempotency-Key` header.
- Repeating the same key with the same canonical route and request returns the original decision result and creates no second decision record.
- Reusing the key with a different canonical route or request returns `409 idempotency-conflict`.
- Without the header, each successful call creates a distinct decision record.
- Exposure confirmation and bundle application require idempotent retry semantics.

Proposed error extension:

```json
{
  "type": "https://flaggo.dev/problems/invalid-inference-input",
  "title": "Invalid inference input",
  "status": 422,
  "detail": "One or more inputs do not conform to the registered decision definition.",
  "instance": "/v1/decisions/tetris.dropInterval:decide",
  "code": "invalid-inference-input",
  "correlationId": "request-789",
  "issues": [
    {
      "code": "signal-type-mismatch",
      "severity": "error",
      "path": "/inputs/0/value",
      "message": "Expected a number for tetris.boardPressure.",
      "signalKey": "tetris.boardPressure"
    }
  ]
}
```

Programs branch on `code`, HTTP status, and structured issue fields, never on `title`, `detail`, or issue messages.

## Runtime API

### Decide

```http
POST /v1/decisions/{decisionKey}:decide
```

Proposed request body:

```json
{
  "expectedContract": {
    "definitionId": "tetris.dropInterval@2",
    "contractDigest": "sha256:contract...",
    "revision": "42",
    "bundleDigest": "sha256:bundle...",
    "buildId": "tetris-web-2026-07-25.1",
    "deploymentId": "tetris-web-dev-a"
  },
  "runtimeTarget": {
    "type": "session",
    "id": "game-456"
  },
  "runtimeContext": {
    "sessionId": "game-456",
    "userId": "user-123",
    "cohort": "new_players",
    "deviceType": "mobile"
  },
  "inputs": [
    {
      "signal": { "key": "tetris.boardPressure" },
      "value": 0.82
    }
  ],
  "client": {
    "appId": "tetris-demo",
    "environment": "dev",
    "sdk": "typescript",
    "sdkVersion": "0.1.0"
  },
  "correlationId": "game-loop-123"
}
```

The HTTP adapter combines the route key and body into the internal canonical `DecideRequest`.

Request invariants:

- `expectedContract` is required and contains `definitionId + revision + contractDigest`.
- Optional `Idempotency-Key` controls retry deduplication; `correlationId` remains tracing metadata and is not uniqueness identity.
- `inputs` contains unique signal keys and is key-sorted by conforming clients.
- The server rejects duplicate signal keys within `inputs`; deterministic ordering does not make conflicting values valid.
- Every input must be declared for the decision's inference role and resolve to an app-emitted primitive metric.
- Runtime context fields must conform to the registered context schema.
- The caller may not submit policy, objectives, action-space changes, strategy definitions, or full decision definitions.
- Browser-supplied contract identity detects drift but is not authorization.

Proposed server response:

```json
{
  "decisionKey": "tetris.dropInterval",
  "definition": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "definitionId": "tetris.dropInterval@2",
    "revision": "42"
  },
  "decisionId": "decision-123",
  "value": 700,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy-tetris-new-players-v1",
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
    "user:user-123",
    "cohort:new_players",
    "global"
  ],
  "confidence": {
    "evidenceQuality": 0.82,
    "modelUncertainty": 0.31,
    "expectedOutcome": 0.72
  },
  "fallback": {
    "source": "server",
    "resolutionFallbackUsed": false,
    "decisionFallbackUsed": false,
    "reason": null
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["cooldown", "max-delta", "number-bounds"]
  },
  "definitionStatus": {
    "revision": "42",
    "contractDigest": "sha256:contract...",
    "bundleDigest": "sha256:bundle...",
    "buildId": "tetris-web-2026-07-25.1",
    "deploymentId": "tetris-web-dev-a",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "exposure": {
    "confirmationRequired": true,
    "confirmToken": "confirm-abc"
  },
  "reason": "Approved strategy slowed the drop interval within policy bounds.",
  "auditId": "audit-789"
}
```

Server response invariants:

- A successful wire response always has `decisionId`, `auditId`, and `fallback.source = "server"`.
- The initial response never has `exposureId`.
- `confidence` is `null` when `decisionFallbackUsed` is true.
- Resolution fallback may still have confidence when a broader target produced an approved decision.
- Set-like arrays are emitted in canonical order; semantically ordered arrays retain their defined order.
- The response contains structured reason codes in policy/fallback fields. Human-readable `reason` is explanatory and must not be used for program logic.
- The default response returns compact confidence and target-resolution provenance. Full evidence-view details remain in the audit record.

The SDK may project this response into `DecisionReceipt<T>` or a detailed result. If the data plane is unavailable and application configuration explicitly enables availability fallback, the SDK creates a distinct client-fallback result with no server `decisionId`, `auditId`, `policy`, `definitionStatus`, or exposure confirmation metadata. It must never do this for a 4xx contract/configuration response.

### Exposure confirmation

```http
POST /v1/exposures/{decisionId}:confirm
```

The decide response records what Flaggo recommended; it does not prove that the application used the value. A response may be discarded because the interaction ended, application state changed, rendering failed, or a newer decision superseded it. Automatically treating every response as an exposure would create false treatment data and bias outcome attribution and future optimization.

Confirmation therefore occurs only after application or rendering:

```text
decision returned -> value applied or rendered -> exposure confirmed -> outcomes attributed
```

Proposed request:

```json
{
  "confirmToken": "confirm-abc",
  "appliedAt": "2026-07-29T19:20:00Z",
  "correlationId": "game-loop-123"
}
```

Proposed response:

```json
{
  "exposureId": "exposure-456",
  "decisionId": "decision-123",
  "status": "confirmed",
  "confirmedAt": "2026-07-29T19:20:01Z"
}
```

Confirmation invariants:

- Confirmation is accepted only for a server decision that requires confirmation.
- Retrying the same valid confirmation returns the same `exposureId`.
- A mismatched token or a conflicting confirmation is rejected.
- `confirmedAt` is server-authoritative; optional `appliedAt` is a client-reported observation and cannot move confirmation outside accepted clock-skew bounds.
- The server copies decision-time inference inputs into the exposure record; the client does not resubmit them.
- Outcome telemetry correlates to `exposureId`, not merely `decisionId`.

## Management API

For MVP, trusted application/bootstrap startup is the first client of these control-plane operations. It uses the same language-neutral validate/apply contract as future CLI, CI/CD, GitOps, init/deployment-hook, and operator clients; no startup-specific management endpoint is introduced.

Startup registration:

1. submits the statically extracted canonical bundle,
2. uses a deterministic idempotency key derived from application, environment, and `bundleDigest`,
3. accepts only an approved registration receipt,
4. initializes the data-plane binding from the receipt,
5. rejects client initialization with a typed error when validation fails or approval is required; no data-plane client is created.

The startup caller requires management authorization. A browser or other untrusted runtime must not contain a long-lived control-plane credential.

### Approve a semantic revision

When apply detects a semantic change under an existing decision key, it returns `status: "requires-approval"` plus an `approvalRequestId` and performs no registry mutation.

```http
GET /v1/definition-bundle-approvals/{approvalRequestId}
POST /v1/definition-bundle-approvals/{approvalRequestId}:approve
POST /v1/definition-bundle-approvals/{approvalRequestId}:reject
```

Approval atomically applies the previously validated canonical bundle and stores the approved registration receipt. The approval status resource returns `pending`, `approved`, or `rejected` and includes the receipt only when approved. A startup retry using the same canonical bundle and deterministic idempotency key returns the approved receipt after approval; before approval it returns the same `requires-approval` result.

### Validate a bundle

```http
POST /v1/definition-bundles:validate
```

Validation is read-only. It canonicalizes the submitted bundle, verifies schemas and references, classifies compatibility, and returns structured issues plus computed digests.

Proposed response:

```json
{
  "status": "valid",
  "bundleDigest": "sha256:bundle...",
  "contractDigest": "sha256:contract...",
  "compatibility": "identical",
  "issues": []
}
```

Each issue should contain a stable `code`, severity, JSON Pointer `path`, human-readable message, and relevant decision or signal key.

### Apply a bundle

```http
POST /v1/definition-bundles:apply
```

Apply repeats validation and then performs an atomic registry change under an idempotency key.

Proposed response:

```json
{
  "application": "tetris-demo",
  "environment": "dev",
  "bundleDigest": "sha256:bundle...",
  "contractDigest": "sha256:contract...",
  "status": "approved",
  "compatibility": "identical",
  "registeredRevisions": {
    "tetris.dropInterval": "42"
  },
  "changes": [],
  "issues": []
}
```

Apply invariants:

- The bundle is the management write unit.
- A repeated request with the same idempotency key and canonical body returns the original result.
- Reusing an idempotency key with a different canonical body is a conflict.
- Missing resources become deprecation candidates; apply never hard-deletes them.
- A semantic conflict never overwrites an immutable definition identity.
- No management mutation occurs from the runtime decide endpoint.
- Failed atomic apply leaves all previously registered resources unchanged and produces no accepted identity for the submitted bundle.
- Semantic change returns `requires-approval` with no mutation. Only the explicit approval operation may authorize and atomically apply that pending canonical bundle.
- Apply failure blocks contract activation, not application deployment. If code is deployed anyway, the data plane rejects its missing or unknown expected identity.

## Authentication and authorization

The OpenAPI contract uses OAuth 2.0/OIDC bearer security with separate scopes:

| Scope | Permits |
| --- | --- |
| `polari.decisions:decide` | Runtime decision evaluation |
| `polari.exposures:confirm` | Exposure confirmation |
| `polari.definitions:validate` | Bundle validation |
| `polari.definitions:apply` | Bundle apply |
| `polari.definitions:approve` | Semantic-revision approval/rejection |

Local development may explicitly enable an unauthenticated bypass. The bypass is configuration, never automatic environment inference. Authorization verifies that the caller may operate on the application/environment declared inside the bundle or runtime client metadata.

## Health API

```http
GET /health/live
GET /health/ready
```

Liveness reports process health. Readiness reports whether mandatory runtime dependencies are available. Health payloads must not expose secrets or detailed internal configuration.

## HTTP outcome model

Proposed baseline:

| Situation | HTTP result |
| --- | --- |
| Approved active value/strategy | `200` server decision |
| Resolution fallback with approved broader target | `200` server decision |
| Governed decision fallback for a known definition | `200` server decision |
| Missing expected contract identity | `400 missing-contract-identity` Problem Details |
| Malformed JSON or duplicate input keys | `400` Problem Details |
| Unknown decision key | `404` Problem Details |
| Unknown definition ID/revision | `409 contract-not-registered` Problem Details |
| Contract digest conflict | `409 contract-conflict` Problem Details |
| Retired definition | `409 retired-definition` Problem Details |
| Schema-valid request with invalid declared input/value | `422` Problem Details |
| Authentication/authorization failure | `401` / `403` Problem Details |
| Rate limit | `429` Problem Details |
| Runtime unavailable before an audited decision exists | `503` Problem Details; SDK may use explicitly configured availability fallback |

The key distinction is whether the server completed an audited evaluation of a valid registered definition. A completed governed fallback is a decision result. Contract/configuration rejection is an actionable error and forbids local fallback. Transport or data-plane availability failure may use explicitly configured local fallback.

## Required golden scenarios

1. Active fixed value.
2. Active numeric strategy.
3. Resolution fallback with confidence.
4. Policy decision fallback with `confidence: null`.
5. Unknown decision key.
6. Missing expected contract identity without local fallback.
7. Unknown expected definition for a known decision key without local fallback.
8. Conflicting contract identity without local fallback.
9. Retired definition without local fallback.
10. Duplicate inference input.
11. Input type/range mismatch.
12. Exposure confirmation and idempotent retry.
13. Invalid exposure token.
14. Valid bundle with identical compatibility.
15. Invalid bundle with structured issues and no mutations.
16. Semantic bundle conflict requiring a new revision.
17. Idempotent bundle apply and key/body conflict.
18. Service-unavailable SDK-local fallback when explicitly enabled.
19. Concurrent startup registration of the same bundle returns one accepted identity.
20. Invalid or approval-pending startup registration does not initialize the data-plane client.
21. Optional decide idempotency replay returns the original decision.
22. Decide idempotency key/body conflict returns `409`.
23. Client cohort claim is verified, accepted as unverified, or replaced with explicit target provenance.
24. Default runtime response omits full evidence-view detail while audit retains it.
25. OAuth scope denial for each runtime and management operation.

## Contract decision log

All Phase 1 product decisions A1-A12 are accepted. The remaining work is to encode them in OpenAPI, JSON Schema, fixtures, and conformance tests.

### A1. Runtime identity minimum

**Status:** Accepted

**Options**

1. Require only `contractDigest`; treat revision and bundle/build identity as diagnostics.
2. Require `definitionId + revision + contractDigest`.
3. Require the full current `ContractIdentity`.

**Decision:** option 2. Require `definitionId + revision + contractDigest`. `definitionId` is the immutable semantic identity; `decisionKey` remains the stable developer-facing lookup key. This verifies content without coupling every runtime request to a whole bundle or deployment.

**Consequence:** option 1 is compact but weakens diagnostics; option 3 increases drift and rolling-deployment complexity.

### A2. Server and client fallback result types

**Status:** Accepted

**Options**

1. Keep one highly optional `DecideResponse` for both server and SDK-local results.
2. Make the server wire result strict and define an SDK `DecisionResult = ServerDecisionResult | ClientFallbackResult`.

**Decision:** option 2. Use a strict server wire result and an SDK union. This prevents client fallback from accidentally claiming server audit, policy, decision, or exposure identity.

**Consequence:** SDK consumers must inspect a discriminant such as `fallback.source`, but generated server types become substantially safer.

### A3. Governed fallback versus HTTP errors

**Status:** Accepted

**Options**

1. Return `200` for every condition when a configured fallback value exists.
2. Return non-2xx for every policy or integrity failure.
3. Return `200` when the server completed an audited evaluation of a valid registered definition; reject malformed, unknown, conflicting, retired, or unavailable requests.

**Decision:** option 3. Return `200` only when the server completed an audited evaluation of a valid registered definition, including governed policy/evidence/state fallback. Reject malformed or contract/configuration failures with non-2xx Problem Details and forbid local fallback for those failures.

**Consequence:** clients distinguish governed server fallback, contract/configuration errors, and availability failures. Unknown or conflicting identity cannot silently use an older registered definition. Only explicitly configured data-plane availability failures may become SDK-local fallback.

### A4. Exposure proof and idempotency

**Status:** Accepted

**Options**

1. Confirm by `decisionId` only.
2. Require the opaque `confirmToken`.
3. Require both a confirm token and a separate `Idempotency-Key`.

**Decision:** option 2 for MVP. Require the opaque `confirmToken`; replaying the same valid confirmation is idempotent and returns the original `exposureId`.

**Consequence:** the token is a capability and must not be logged; option 3 is stronger for complex clients but adds state and SDK surface.

### A5. Bundle apply atomicity

**Status:** Accepted

**Options**

1. Apply valid entries and return per-entry failures.
2. Validate and apply the entire bundle atomically.

**Decision:** option 2. Validate and apply the entire bundle atomically. A build should not deploy against a partially registered ownership manifest.

**Consequence:** one invalid definition blocks unrelated control-plane changes in the same bundle; teams may need smaller ownership bundles later. It does not block external application deployment, but deployed code cannot use an unapplied identity.

### A6. Semantic revision creation

**Status:** Accepted

**Options**

1. Apply automatically mints semantic revisions when the submitted key changed.
2. Apply returns `requires-approval` and makes no mutation until an explicit approval operation.
3. The client must provide a new explicit definition ID before apply.

**Decision:** option 2. Apply returns `requires-approval` and makes no mutation. The approval resource records review state; explicit approval atomically applies the pending canonical bundle and produces the registration receipt.

**Consequence:** under MVP startup registration, `requires-approval` rejects initialization. After approval, startup retries or restarts and receives the accepted binding for the same canonical bundle.

### A7. Environment placement

**Status:** Accepted

**Options**

1. Keep application/environment only inside the bundle and runtime client metadata.
2. Put application/environment in management URL paths.
3. Resolve them exclusively from authentication credentials.

**Decision:** option 1. Application and environment remain inside the portable bundle and runtime client metadata. Authorization verifies that the caller may access the declared scope.

**Consequence:** body scope must be checked against credentials; path scoping is more REST-visible but duplicates bundle identity.

### A8. Cohort/segment authority

**Status:** Accepted

**Options**

1. Trust client-provided cohort membership.
2. Resolve cohort membership only on the server.
3. Accept client claims as inputs but verify or replace them server-side when authoritative resolution exists.

**Decision:** option 3. Accept client cohort claims as context, but verify or replace them when authoritative server resolution exists. Browser claims are not an authorization or governance boundary.

**Consequence:** response/audit must record whether membership was claimed, verified, server-derived, or server-replaced.

### A9. Evidence detail in runtime responses

**Status:** Accepted

**Options**

1. Return full evidence-view references and confidence details.
2. Return only compact confidence and target provenance; keep evidence views in audit.

**Decision:** option 2. The default response returns compact confidence and target provenance; full evidence-view references remain in audit. A diagnostic expansion may be designed later.

**Consequence:** smaller payloads and less leakage, but debugging requires audit access.

### A10. Batch decisions

**Status:** Accepted

**Options**

1. Include a batch endpoint in v1.
2. Defer batch until single-decision semantics and exposure attribution are stable.

**Decision:** option 2. Batch decisions are deferred until single-decision and exposure semantics are stable.

**Consequence:** applications initially make multiple calls; transport optimization does not delay the core contract freeze.

### A11. Authentication contract

**Status:** Accepted

**Options**

1. Exclude authentication from v1 and bind only to a trusted local network.
2. Define API-key security for runtime and management APIs.
3. Define OAuth 2.0/OIDC bearer scopes, with a pluggable local-development bypass.

**Decision:** option 3. OpenAPI defines OAuth 2.0/OIDC bearer scopes for decide, exposure confirmation, validation, apply, and approval. MVP implementation may start with an explicit local-development bypass.

**Consequence:** this preserves a production-safe boundary but requires an identity/authorization design before management APIs can be exposed outside local development.

### A12. Decide retry identity

**Status:** Accepted

**Options**

1. Every retry creates a new decision record.
2. Require an idempotency key on every decide call.
3. Support an optional idempotency key: repeated key plus canonical request returns the original decision; calls without it create new decisions.

**Decision:** option 3. `Idempotency-Key` is optional. Same key plus canonical request returns the original decision; omission creates a new decision; key reuse with different request content is a conflict.

**Consequence:** SDKs can safely retry transport failures without conflating normal repeated game-loop decisions, but the service needs bounded idempotency storage and conflict behavior.

## Freeze exit criteria

The proposal can move from draft to frozen only when:

- A1-A12 are represented consistently in the executable artifacts.
- Runtime and management OpenAPI documents validate.
- The bundle JSON Schema validates canonical examples.
- Every required golden scenario has an expected request/result.
- SDK and service can generate or consume types without handwritten wire DTO divergence.
- Basic and advanced SDK authoring forms generate the same canonical definition fixture.
- Contract changes have an explicit compatibility and versioning process.
