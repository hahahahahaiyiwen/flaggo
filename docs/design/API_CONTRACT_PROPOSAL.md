# Phase 1 API Contract Proposal

Status: **Accepted Phase 1 runtime baseline; bundle management replacement approved**

This proposal is the accepted prose design source for Phase 1. The executable
baseline under [`contracts/`](../../contracts/README.md) encodes it and passes
the conformance gate. Later behavior changes require an explicit contract
revision with aligned schemas, OpenAPI, fixtures, and compatibility notes.

This document retains current executable field examples where needed to
describe the frozen Phase 1 API. The canonical target names are defined in the
[Shared Contracts executable migration map](shared-contracts/README.md#executable-migration-map);
issue #40 replaces those names atomically rather than supporting both.

It does not introduce a second domain model. Canonical domain types remain owned by [Shared Contracts](shared-contracts/README.md); this document defines how those types cross HTTP and build/release boundaries.

Roadmap note: the runtime decision and exposure endpoint shapes remain the
accepted baseline. The definition-bundle v1 management shape and the rule that
every strategy result requires confidence are superseded by the approved
bundle-authority design. The replacement permits `confidence: null` for a
deterministic bundle-authored strategy that makes no evidence-backed claim.
The follow-up changes these contracts and executable artifacts together, with
no v1/v2 bundle compatibility layer. Until that work lands, the artifacts
listed below describe the current executable repository rather than the target
contract.

Compatibility note (2026-08-06): exposure confirmation now explicitly
documents the already implemented `500 internal-error` and retryable
`503 service-unavailable` outcomes for durable audit/infrastructure failure.
This is an additive OpenAPI response declaration. It does not make confirmation
audit failures eligible for SDK-local fallback: `500` has no eligibility
extension, while the generic I/O `503` is explicitly ineligible and carries
matching `Retry-After`/`retryAfterSeconds` metadata.

## Goals

- Let TypeScript client and Decision Service teams implement in parallel from one contract revision.
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

Flaggo uses a cloud-service boundary:

| Boundary | Phase 1 behavior |
| --- | --- |
| Contract Service | Definition-bundle validate/apply and immutable contract lifecycle. |
| Data plane | Decide and exposure confirmation for exact registered identities. |
| Application deployment | Developer-owned and independent from Flaggo. |

For MVP, trusted application/bootstrap startup uses Contract Service to
atomically register the statically extracted bundle before enabling Decision
Service calls. Future clients may publish manually or integrate SDK/CLI tooling
into CI/CD, GitOps, release pipelines, init/deployment hooks, verify-only
startup, or contract-first workflows. Flaggo does not claim to block external
code deployment.

Code deployed without a registered binding can still run. Startup registration may establish that binding; if it is skipped or fails, decision calls remain disabled. The data plane never registers from decide traffic, silently selects an older revision, or converts a contract/configuration error into local fallback.

Detailed developer UX: [Control Plane and Data Plane UX](client-library/CONTROL_DATA_PLANE_UX.md).

## Sources of truth

| Concern | Authority |
| --- | --- |
| Domain types and invariants | [Shared Contracts](shared-contracts/README.md) |
| Runtime HTTP behavior and orchestration | [Decision Service](decision-service/README.md) |
| SDK authoring and result projection | [Client Library](client-library/README.md) |
| Bundle lifecycle and compatibility | [Contract Service](contract-service/README.md) |
| Exposure and attribution semantics | [Evidence](../architecture/EVIDENCE.md) |
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
    problem-details-v1.schema.json
  fixtures/
    runtime/
      decide/
      exposure-confirmation/
      health/
    management/
      definition-bundle/
      approvals/
    errors/
  conformance/
    fixture-manifest-v1.json
    generated-model-compatibility/
  mock/
    fixture-server/
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
- Timestamps use RFC 3339 UTC strings with the `Z` suffix; numeric offsets are rejected.

### Runtime identity semantics

- `decisionKey` is the stable developer-facing lookup key within an application and environment.
- `definitionId` is an opaque registry-issued lineage ID. It is stable across approved semantic revisions and carries no parseable version meaning.
- `revision` is an opaque registry-issued runtime revision ID. It is neither semantic versioning nor a mutable metadata revision.
- `contractDigest` is the SHA-256 digest of the RFC 8785 canonical semantic definition for that revision.
- The immutable runtime identity is the complete `{ definitionId, revision, contractDigest }` tuple. No member is sufficient alone.
- Metadata-only edits are recorded in registry/audit history but do not create a runtime revision or change `contractDigest`.
- Approval of a semantic change atomically creates a new `revision` and `contractDigest` under the existing `definitionId`. The previous tuple remains addressable while lifecycle policy permits it.
- A new `definitionId` is created only for a new decision lineage or an explicit fork, never by encoding a version into an ID.
- Definition normalization regenerates signal associations only from semantic roles, sorts set-like collections, removes generated identity fields and definition metadata, canonicalizes default-valued options, and then hashes RFC 8785 bytes.
- Each signal declaration's `schemaDigest` is `sha256:<lowercase-hex>` over its RFC 8785 canonical declaration with `schemaDigest` omitted. Supplied digests are recomputed and verified; the same signal key with a different computed digest is a contract conflict.
- `bundleDigest` preserves submitted bundle metadata for artifact identity, but normalizes signal declarations and every definition before sorting definitions by `decisionKey` and hashing.

### Media types

- Domain result payloads use `application/json`, including non-ready
  lifecycle results returned by bundle apply on `409` or `503`.
- Errors use `application/problem+json` following RFC 9457 Problem Details.
- When one operation can return either a lifecycle result or an error at the
  same status, its OpenAPI response must declare both media types. Clients
  select the schema from `Content-Type` before inspecting the body
  discriminator or Problem Details `code`.
- Unknown JSON fields are rejected on management write APIs and ignored only where the OpenAPI contract explicitly permits forward-compatible extension.

### Correlation and retries

- Clients may send `X-Flaggo-Correlation-Id` only as an HTTP header. Body `correlationId` fields are forbidden.
- The HTTP adapter maps the resolved header value into the internal domain request's `correlationId`; when absent, the service generates one.
- The service returns the resolved `X-Flaggo-Correlation-Id` on every response, including liveness and readiness responses.
- Decide callers may send an optional `Idempotency-Key` header.
- Without the header, each successful call creates a distinct decision record.
- Exposure confirmation and bundle application require idempotent retry semantics.

For decide, the idempotency namespace is:

```text
authorized tenant/application/environment
  + OpenAPI operation ID
  + normalized decisionKey
  + Idempotency-Key
```

The request fingerprint is SHA-256 over the HTTP method, normalized route template and parameters, negotiated API version/media type, and RFC 8785 canonical JSON body. Authorization credentials, `X-Flaggo-Correlation-Id`, trace headers, and other tracing-only metadata are excluded from the fingerprint but remain part of the authorization namespace.

Decide idempotency behavior:

- The first request atomically claims the namespaced key before evaluation.
- Concurrent requests with the same fingerprint coalesce and return the same terminal response. If the original does not finish within the request wait budget, followers receive `409 idempotency-in-progress` with `Retry-After` and retry the same key.
- Reusing the key with a different fingerprint returns `409 idempotency-conflict`.
- Successful `200` results and deterministic `4xx` results are retained for 24 hours from completion. The response includes `Idempotency-Key-Expires-At`.
- A `5xx` produced before a decision/audit record exists releases the claim. An audited governed fallback is a `200` and remains retained.
- Expired keys may be reused as new requests; clients that need a longer deduplication window must persist the original decision result.

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
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "contractDigest": "sha256:contract...",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
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
    "cohort": "new_players"
  },
  "inputs": [
    {
      "signal": { "key": "tetris.boardPressure" },
      "value": 0.82
    },
    {
      "signal": { "key": "tetris.currentLevel" },
      "value": 7
    },
    {
      "signal": { "key": "tetris.recentPlacementTimeMs" },
      "value": 1420
    },
    {
      "signal": { "key": "tetris.recoveryFailures" },
      "value": 2
    }
  ],
  "client": {
    "appId": "tetris-demo",
    "environment": "dev",
    "sdk": "typescript",
    "sdkVersion": "0.1.0"
  }
}
```

The HTTP adapter combines the route key and body into the internal canonical `DecideRequest` and maps `X-Flaggo-Correlation-Id` into its tracing metadata.

Request invariants:

- `expectedContract` is required and contains `definitionId + revision + contractDigest`.
- Optional `Idempotency-Key` controls retry deduplication; the correlation header remains tracing metadata and is not uniqueness identity.
- `inputs` contains unique signal keys and is key-sorted by conforming clients.
- The server rejects duplicate signal keys within `inputs`; deterministic ordering does not make conflicting values valid.
- Every required `inference.inputs` signal is present exactly once.
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
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "decisionId": "decision-123",
  "value": 850,
  "valueType": "number",
  "decisionMode": "strategy",
  "strategyId": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
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
    "deploymentId": "tetris-web-dev-a",
    "integrity": "verified",
    "compatibility": "identical"
  },
  "exposure": {
    "confirmationRequired": true,
    "confirmToken": "confirm-abc"
  },
  "reason": "The approved weighted numeric rule met its 0.55 threshold.",
  "auditId": "audit-789"
}
```

Server response invariants:

- A successful wire response always has `decisionId`, `auditId`, and `fallback.source = "server"`.
- A successful wire response always repeats the exact accepted `definitionId + revision + contractDigest` and has `definitionStatus.integrity = "verified"`.
- `runtimeTarget` and `controlTarget` are optional. Global or otherwise targetless decisions omit them and return an empty provenance list with a resolution chain ending at `global`.
- `controlTarget` and `strategyId` are omitted when no authority was selected.
  A `missing_state` fallback then returns empty authority provenance while
  retaining the ordered attempted targets in `resolutionChain`.
- `valueType` and `value` form a discriminated union: boolean with boolean, number with finite JSON number, and string with string.
- The initial response never has `exposureId`.
- `confidence` is `null` when `decisionFallbackUsed` is true and may also be null for a non-evidence-based `active-value`.
- `confidence` is non-null only when the selected authority makes an
  evidence-backed claim. Deterministic bundle-authored strategies and server
  decision fallback return `null`; an evidence-backed broader-target decision
  retains its confidence.
- A non-null confidence object requires `evidenceQuality`; an empty object is invalid. Every confidence field is in the inclusive range `[0, 1]`, and higher `modelUncertainty` means less certainty.
- Exposure metadata is a union. `confirmationRequired: true` requires `confirmToken`; `confirmationRequired: false` forbids it.
- Server decision fallback returns `confirmationRequired: false`; it is not an
  exposure-eligible authority decision.
- `resolutionFallbackUsed` is true only when an active authority was selected
  from a broader permitted target, not when every target missed.
- Set-like arrays are emitted in canonical order; semantically ordered arrays retain their defined order.
- The response contains structured reason codes in policy/fallback fields. Human-readable `reason` is explanatory and must not be used for program logic.
- A blocked policy never approves its candidate. When policy supplies the governed fallback value, the `200` response uses `decisionMode: "fallback"`, `decisionFallbackUsed: true`, and may report `policy.result: "blocked"`.
- Pending activation, failed readiness, or corrupt/incoherent state is an error,
  not a successful `missing_state` fallback.
- The default response returns compact confidence and target-resolution provenance. Full evidence-view details remain in the audit record.

The SDK may project this response into `DecisionReceipt<T>` or a detailed result. If the data plane is unavailable and application configuration explicitly enables availability fallback, the SDK creates a distinct client-fallback result with no server `decisionId`, `auditId`, `policy`, `definitionStatus`, or exposure confirmation metadata. It must never do this for a 4xx contract/configuration response.

### SDK availability-fallback classifier

Availability fallback is disabled by default. When enabled, it is eligible only after the SDK exhausts its retry policy for:

- DNS failure, connection refusal/reset, or connection/read timeout before a complete HTTP response,
- intermediary HTTP `502` or `504` responses,
- a valid Flaggo `5xx` Problem Details response only when `clientFallback.eligible` is explicitly `true`.

It is forbidden for TLS/certificate validation failures, proxy/authentication configuration failures, cancellation requested by application code, malformed responses, every HTTP `503` without explicit `clientFallback.eligible: true`, HTTP `4xx` including `408` and `429`, HTTP `500`, `501`, or `505`, every contract/configuration error, and every valid Flaggo Problem Details response whose eligibility is false or absent.

Definition and authority readiness failures have dedicated outcomes and never
reuse fallback-eligible `service-unavailable`:

- `409 definition-not-ready` means the exact contract is known but its required
  initial activation is pending or failed;
- `503 decision-service-not-ready` means a required state, policy, or audit
  readiness check failed or cannot be read safely;
- `500 invalid-decision-state` means a persisted state violates canonical
  invariants.

`definition-not-ready` is a contract/readiness error. The two `5xx`
readiness/integrity errors include `clientFallback.eligible: false`; the SDK
surfaces all three without a server or local fallback.

`required-evidence-unavailable` is an evaluation or policy outcome, not an
availability failure. When governed fallback is permitted, the service returns
the registered fallback as an audited `200` server decision. Otherwise it
returns fallback-ineligible Problem Details:

```json
{
  "type": "https://flaggo.dev/problems/required-evidence-unavailable",
  "title": "Required evidence unavailable",
  "status": 503,
  "code": "required-evidence-unavailable",
  "clientFallback": {
    "eligible": false
  }
}
```

The SDK surfaces this problem and must not return a local value.
`service-unavailable` sets `eligible: true` only for genuine transient
data-plane transport, capacity, or dependency availability failure after all
required readiness checks passed. It must not represent pending activation,
corrupt/incoherent state, or failed state/audit readiness. The SDK's own
availability-fallback configuration is still required in every eligible case,
so server permission cannot enable fallback by itself.

The Phase 1 SDK default is one retry after the initial attempt, using the same `Idempotency-Key`. It honors `Retry-After` up to one second; otherwise it waits a randomized 50–150 ms. Applications may configure zero, one, or two retries, but fallback cannot occur before the configured attempts are exhausted.

Before either remote evaluation or availability fallback, the generated local call-site `contractDigest` must equal the digest in the accepted runtime binding for that `decisionKey`. Missing binding or mismatch is a local contract error and forbids both the network call and local fallback. A `ClientFallbackResult` may report that accepted expected identity as client provenance, but cannot claim server evaluation, policy, audit, decision, or exposure identity.

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
  "appliedAt": "2026-07-29T19:20:00Z"
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
3. accepts only a ready registration receipt,
4. initializes the data-plane binding from the receipt,
5. rejects client initialization with a typed error while approval or
   activation is pending, failed, or rejected; no data-plane client is created.

The startup caller requires management authorization. A browser or other untrusted runtime must not contain a long-lived control-plane credential.

### Approve a new or semantic revision, or reauthorize authority

When apply detects a previously unknown decision key, a semantic change under
an existing key, or an exact-bundle retry after permanent activation failure,
it returns `status: "requires-approval"` plus an `approvalRequestId`. Apply
persists the immutable request and may reserve a proposed lineage ID, but it
does not publish an accepted runtime revision or mutate active authority.

Apply returns the canonical `DefinitionBundleApplyResult` with these HTTP
mappings:

- `200` for `status: "ready"`;
- `202` for `requires-approval` or `activation-pending`;
- `503` plus `Retry-After` for retryable `activation-failed`;
- `409` for `activation-failed` with `requires-new-approval` or
  `approval-rejected`;
- `422 invalid-bundle` Problem Details when validation rejects the write.

Every listed `DefinitionBundleApplyResult` body uses `application/json`,
including the typed `409` and `503` outcomes. Authentication, idempotency,
validation, and infrastructure failures use `application/problem+json`; the
apply operation must declare both media types on shared statuses.

The `202 requires-approval` body is:

```json
{
  "status": "requires-approval",
  "approvalRequestId": "apr_01JQ91C2D3E4F5G6H7J8K9M0N1",
  "application": "tetris-demo",
  "environment": "dev",
  "bundleDigest": "sha256:bundle...",
  "compatibility": "new-contract-required",
  "expiresAt": "2026-08-06T18:30:00Z",
  "changes": [
    {
      "kind": "semantic-change",
      "decisionKey": "tetris.dropInterval",
      "previous": {
        "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
        "revision": "rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K",
        "contractDigest": "sha256:old-contract..."
      },
      "proposed": {
        "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
        "contractDigest": "sha256:new-contract..."
      },
      "semanticDiff": [
        {
          "op": "add",
          "path": "/inference/inputs/2",
          "after": { "key": "tetris.recoveryFailures" }
        }
      ]
    }
  ],
  "snapshotUrl": "/v1/definition-bundle-approvals/apr_01JQ91C2D3E4F5G6H7J8K9M0N1/bundle",
  "issues": []
}
```

```http
GET /v1/definition-bundle-approvals/{approvalRequestId}
GET /v1/definition-bundle-approvals/{approvalRequestId}/bundle
POST /v1/definition-bundle-approvals/{approvalRequestId}:approve
POST /v1/definition-bundle-approvals/{approvalRequestId}:reject
```

Approve request:

```json
{
  "expectedBundleDigest": "sha256:bundle...",
  "comment": "Reviewed signal and policy changes."
}
```

Reject request:

```json
{
  "expectedBundleDigest": "sha256:bundle...",
  "reasonCode": "operator-rejected",
  "comment": "The new required signal has not completed warmup."
}
```

Every approval resource variant requires `approvalRequestId`, `application`,
`environment`, `bundleDigest`, `createdAt`, `expiresAt`, `changes`, and
`snapshotUrl`. `changes` is non-empty and contains at least one `created`,
`semantic-change`, or `authority-reauthorization` entry; every semantic change
carries a non-empty canonical `semanticDiff`. A renewed request also carries
`supersedesApprovalRequestId`. Its status-specific fields form a discriminated
union:

- `pending` has no receipt or terminal decision.
- `approved` requires `decidedAt`, approval metadata containing the
  server-derived actor plus optional persisted comment, and the canonical
  activation projection:
  - `pending` carries only still-pending initial-authority activation plans;
  - `ready` carries the complete `RegistrationReceipt`, either immediately
    after approved proposal-managed publication or after every required
    bundle-approved activation succeeds;
  - `failed` carries only failed or unresolved plans, aggregate retryability,
    and non-empty stable issues keyed by decision.
- `rejected` requires `decidedAt` and rejection metadata containing the server-derived actor, `reasonCode`, and optional persisted comment.
- `expired` requires `expiredAt` and has no receipt. Expiration is terminal.

`GET` and a successful terminal action return `200` with that union. Approve
returns `approved` only after the approval decision, reserved lineage and
allocated revision identities, and captured expected baselines are durably
committed; state activation may still be pending or failed. Clients initialize
runtime bindings only from
`activation.status = "ready"` and its exact bundle-apply receipt.

`GET .../{approvalRequestId}/bundle` returns the exact canonical
`DecisionDefinitionBundle` snapshot used to calculate `bundleDigest`. It is
immutable across every approval state and retained with the approval audit
record. `changes` contains a proposed identity for creation, previous and
proposed identities plus canonical semantic diff for semantic change, or the
exact accepted identity, target, and failed activation reference for authority
reauthorization. Semantic diffs are sorted by JSON Pointer path and use `add`,
`remove`, and `replace` over compatibility-critical canonical content; they
exclude build metadata and other fields outside `contractDigest`.

The snapshot headers represent the same canonical bytes using their protocol-specific syntax:

```http
ETag: "sha256:<lowercase-hex>"
Content-Digest: sha-256=:<base64-of-raw-sha256-bytes>:
```

`ETag` is a quoted opaque entity tag. `Content-Digest` follows RFC 9530 Structured Fields syntax and base64-encodes the raw 32-byte SHA-256 result; it never embeds the project `sha256:<hex>` string directly.

Approval behavior:

- Apply stores an immutable canonical bundle snapshot behind the approval request. Approval never re-reads mutable client content.
- Approval actor identity is derived from the authorized token (`sub` plus optional display name), never accepted from the request body. Explicit local bypass records actor subject `local-development`.
- The MVP default expiration is seven days; every response carries the authoritative `expiresAt`.
- Approve verifies `expectedBundleDigest`, then atomically transitions
  `pending -> approved`, verifies the reserved proposed lineage identities,
  allocates and publishes runtime revisions for created or semantic changes,
  reuses the existing revision for authority reauthorization, and stores each
  required initial-authority activation plan with the stable-head baseline
  captured at that transition. If no initial authority is declared, approved
  publication stores the ready receipt immediately. Otherwise activation may
  complete afterward, and only completion stores the ready receipt.
- Reject verifies `expectedBundleDigest`, then atomically transitions
  `pending -> rejected` without runtime publication or authority mutation.
- Repeating the same terminal action with the same digest is idempotent and returns the stored result. No separate idempotency key is required.
- Concurrent terminal actions use one compare-and-swap transition. One wins; the same action converges on its result, while the opposite action returns `409 approval-terminal-conflict`.
- An unknown request returns `404 approval-not-found`. `GET` returns an expired resource as `200`; approve/reject against it return `410 approval-expired`. A digest mismatch returns `409 approval-bundle-conflict`.
- Terminal states never transition again. Approval/rejection authorization and application/environment scope are checked on every action.
- A startup retry using the same canonical bundle and deterministic apply
  idempotency key returns `requires-approval` before approval,
  `activation-pending` or `activation-failed` while non-ready, and the stored
  ready receipt after approved publication when no activation is required or
  after every required activation succeeds.
- After `activation-failed` with `requires-new-approval`, the next exact
  reapply atomically advances that deterministic registration attempt to one
  linked `requires-approval` resource. Concurrent reapplies converge on it.
  Its changes reauthorize only failed initial authorities against newly
  captured baselines, reuse the accepted definition revision and successful
  partial activations, and do not create another semantic revision. Its ready
  receipt combines those new activations with every accepted definition and
  successful activation retained from the original bundle. Because the
  canonical bundle is unchanged, this request reports
  `compatibility: "identical"`; approval authorizes a new baseline and
  activation rather than new contract semantics.
- After expiration, the next apply of the same canonical body with the same deterministic key revalidates against current registry state and atomically creates one fresh approval request with a new ID and expiry. Concurrent resubmissions converge on that request. The old request remains `expired`, and the new resource links it through `supersedesApprovalRequestId`.

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
  "compatibility": "identical",
  "validatedDefinitions": {
    "tetris.dropInterval": {
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
      "contractDigest": "sha256:contract..."
    }
  },
  "issues": []
}
```

Each issue should contain a stable `code`, severity, JSON Pointer `path`, human-readable message, and relevant decision or signal key.

Validation results are a discriminated union. `status: "valid"` requires `bundleDigest`, `compatibility`, a non-empty `validatedDefinitions` map, and `issues`. `status: "invalid"` requires at least one error issue; digest and partial validation fields are optional when canonicalization reached them.

`requires-approval` is also strict: it requires at least one `created`,
`semantic-change`, or `authority-reauthorization` entry, every semantic change
requires a non-empty `semanticDiff`, and `issues` may contain warnings only.
Approval resources preserve that approval-requiring change invariant in every
lifecycle state. Compatibility is `new-contract-required` when any change is
`created` or `semantic-change`, and `identical` only when every change is
`authority-reauthorization`.

Each definition entry is also a discriminated union keyed by `valueType`. Boolean, number, and string entries require matching action-space defaults and fallback value types. Numeric bounds must ascend, defaults and fallbacks must be in range, `step` must be positive, and numeric defaults/fallbacks must align to it. String defaults and fallbacks must belong to `allowedValues` when that set is present.

Optional `definitionId` rules:

- Omission is required for a new decision key. Validation computes its
  proposed `contractDigest`; apply reserves and persists the new opaque lineage
  ID in the approval request, while approval allocates and publishes the
  initial runtime revision.
- Omission is valid for a known key. Validation resolves that key's existing lineage within the declared application/environment and compares semantics against its active revision.
- A supplied ID is valid only when it is the existing lineage for the same authorized application, environment, and decision key.
- A supplied unknown registry ID returns `unknown-definition-lineage`. An ID owned by another key or scope returns `definition-lineage-mismatch` without disclosing the other owner.
- The registry never adopts a caller-invented ID. These failures make validation `status: "invalid"` and make apply return `422 invalid-bundle`.

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
  "status": "ready",
  "compatibility": "identical",
  "acceptedDefinitions": {
    "tetris.dropInterval": {
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
      "contractDigest": "sha256:contract...",
      "activatedAuthority": {
        "proposalId": "proposal_01...",
        "activationId": "activation_01...",
        "strategyId": "strategy_01...",
        "stateId": "state_01...",
        "generation": 1,
        "controlTarget": {
          "type": "cohort",
          "id": "new_players"
        },
        "kind": "numeric-rule"
      }
    }
  },
  "changes": [],
  "issues": []
}
```

The example shows a `numeric-rule` receipt. `activatedAuthority` is
discriminated by `kind`: `numeric-rule` requires `strategyId`, while
`active-value` forbids `strategyId` and identifies the same proposal,
activation, state, generation, and control target lineage.

Apply invariants:

- The bundle is the management write unit.
- `acceptedDefinitions` is the only source for initializing per-decision runtime bindings; clients must not construct identities from the bundle digest or decision key.
- A repeated request with the same idempotency key and canonical body returns
  the current result for the same approval and activation plan: pending
  approval, activation pending/failed, ready, or rejected. A permanent
  activation failure advances to the linked authority-reauthorization request
  described above; expiration advances to the linked replacement behavior.
- Reusing an idempotency key with a different canonical body is a conflict.
- Missing resources become deprecation candidates; apply never hard-deletes them.
- A semantic conflict never overwrites an immutable definition identity.
- No management mutation occurs from the runtime decide endpoint.
- Failed atomic apply leaves all previously registered resources unchanged and produces no accepted identity for the submitted bundle.
- A newly created definition or semantic change returns `requires-approval`
  without publishing an accepted runtime revision. Only explicit approval may
  authorize and atomically publish the pending canonical bundle; apply may
  persist its immutable approval request and proposed lineage identity.
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

Liveness response:

```json
{
  "status": "live",
  "service": "flaggo",
  "version": "0.1.0",
  "observedAt": "2026-07-30T18:30:00Z"
}
```

`/health/live` returns `200` when the process can serve the handler. It performs no external dependency checks. A hung or terminated process is detected by timeout/no response rather than a special payload.

Readiness response:

```json
{
  "status": "ready",
  "observedAt": "2026-07-30T18:30:00Z",
  "checks": [
    { "name": "contract-registry", "status": "up", "required": true },
    { "name": "decision-state", "status": "up", "required": true },
    { "name": "policy", "status": "up", "required": true },
    { "name": "audit", "status": "up", "required": true },
    { "name": "evidence", "status": "up", "required": false }
  ]
}
```

Readiness semantics:

- Top-level status is `ready`, `degraded`, or `not-ready`.
- Check status is `up`, `degraded`, or `down`; `required` is globally fixed by service configuration and included in every check.
- Any required check that is not `up` produces `503` and `not-ready`.
- Optional checks may be `degraded` or `down` while the endpoint returns `200 degraded`, but only when the runtime can still produce an audited governed fallback.
- `200 ready` requires every check to be `up`.
- The Phase 1 required checks are Contract Store, State Store, decision
  constraints, and durable Evidence Store record append. Evidence projections
  are globally optional: their loss always yields `200 degraded` readiness and
  never dynamically changes the check's `required` flag.
- A ready data plane binds a durable audit sink. Successful audit completion means the record crossed that sink's durability boundary and survives process failure; console and in-memory sinks are limited to tests or explicitly non-ready debugging modes.
- Evidence requirements are enforced per decide request. When an exact definition requires unavailable evidence, the server returns an audited governed fallback if its policy permits one; otherwise it returns `503 required-evidence-unavailable`. This request outcome does not change global readiness semantics.
- Readiness performs no mutation and discloses only stable dependency names and coarse states. It never returns connection strings, exception text, hostnames, credentials, or detailed configuration.

## HTTP outcome model

Proposed baseline:

| Situation | HTTP result |
| --- | --- |
| Approved active value/strategy | `200` server decision |
| Resolution fallback with approved broader target | `200` server decision |
| Governed decision fallback for a known definition | `200` server decision |
| Missing expected contract identity | `400 missing-contract-identity` Problem Details |
| Malformed JSON | `400 malformed-json` Problem Details |
| Duplicate input keys | `400 duplicate-signal-input` Problem Details |
| Unknown decision key | `404 unknown-decision-key` Problem Details |
| Unknown definition ID/revision | `409 contract-not-registered` Problem Details |
| Contract digest conflict | `409 contract-conflict` Problem Details |
| Retired definition | `409 retired-definition` Problem Details |
| Known definition whose required activation is pending or failed | `409 definition-not-ready` Problem Details; client fallback forbidden |
| Invalid runtime context | `422 invalid-runtime-context` Problem Details |
| Invalid declared input/value | `422 invalid-inference-input` Problem Details |
| A required state, policy, or audit readiness check failed | `503 decision-service-not-ready` Problem Details with `clientFallback.eligible: false` |
| Persisted state violates canonical invariants | `500 invalid-decision-state` Problem Details with `clientFallback.eligible: false` |
| Missing or invalid credentials | `401 authentication-required` Problem Details |
| Missing operation scope | `403 insufficient-scope` Problem Details |
| Credential/body application or environment mismatch | `403 scope-mismatch` Problem Details |
| Decide key reused with another fingerprint | `409 idempotency-conflict` Problem Details |
| Matching decide request still executing after wait budget | `409 idempotency-in-progress` Problem Details plus `Retry-After` |
| Rate limit | `429 rate-limited` Problem Details |
| Definition requires evidence that is currently unavailable and policy forbids governed fallback | `503 required-evidence-unavailable` with `clientFallback.eligible: false`; SDK-local fallback is forbidden |
| Genuine transient data-plane availability failure after readiness passed and before an audited decision exists | `503 service-unavailable` Problem Details with `clientFallback.eligible: true`; SDK may use explicitly configured availability fallback |

The key distinction is whether the server completed an audited evaluation of a
valid registered definition. A completed governed fallback is a decision
result. Contract, activation, state-integrity, and readiness rejections are
actionable errors and forbid local fallback. Only genuine transient transport
or data-plane availability failure may use explicitly configured local
fallback.

### Stable management and exposure errors

| Situation | HTTP result |
| --- | --- |
| Unsupported media type | `415 unsupported-media-type` |
| Structurally processable but invalid bundle on apply | `422 invalid-bundle` with structured issues |
| Bundle key/body conflict | `409 bundle-idempotency-conflict` |
| Approval request not found | `404 approval-not-found` |
| Approval request expired | `410 approval-expired` |
| Approval expected digest mismatch | `409 approval-bundle-conflict` |
| Opposite terminal approval action | `409 approval-terminal-conflict` |
| Decision or confirmation target not found | `404 exposure-not-found` |
| Invalid or mismatched confirmation capability | `404 exposure-not-found` to avoid a token-validity oracle |
| Conflicting exposure confirmation | `409 exposure-confirmation-conflict` |
| Invalid `appliedAt` or clock skew | `422 invalid-applied-at` |
| Durable exposure audit unavailable | `503 service-unavailable` with `Retry-After`; `clientFallback.eligible` is `false` |
| Unexpected exposure confirmation infrastructure failure | `500 internal-error`; no client fallback eligibility |

Validation returns `200` with `status: "invalid"` when a well-formed bundle can be analyzed. Apply returns `422 invalid-bundle` for the same invalid content because no write can occur. Phase 1 `ContractIssue.code` values include:

| Issue code | Meaning |
| --- | --- |
| `invalid-definition` | Definition shape or invariant is invalid. |
| `unknown-definition-lineage` | A caller supplied an ID not issued by the registry. |
| `definition-lineage-mismatch` | A supplied ID is not the lineage for this authorized key/scope. |
| `duplicate-decision-key` | The bundle contains conflicting entries for one decision key. |
| `invalid-signal-schema` | A signal declaration has invalid type/schema semantics. |
| `signal-schema-conflict` | One immutable signal key maps to conflicting schemas. |
| `unknown-signal` | A referenced signal is not declared. |
| `invalid-inference-input` | An inference input is unsupported or incompatible. |
| `invalid-objective` | Objective type, direction, target, or signal is invalid. |
| `invalid-policy` | Policy reference or inline constraint is invalid. |
| `invalid-strategy` | Strategy declaration is unsupported or incompatible. |
| `invalid-reference` | Another bundle reference cannot be resolved. |

New issue codes may be added compatibly, but existing meanings and HTTP mappings cannot change within v1. Clients branch on `code` and structured fields, never messages.

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
16. New definition or semantic bundle change returns `202 requires-approval`
    without accepted runtime publication.
17. Idempotent bundle apply and key/body conflict.
18. Eligible post-readiness `503 service-unavailable` SDK-local fallback for genuine transient availability after retry exhaustion when explicitly enabled.
19. Concurrent startup registration of the same bundle returns one accepted identity.
20. Invalid or approval-pending startup registration does not initialize the data-plane client.
21. Optional decide idempotency replay returns the original decision.
22. Decide idempotency key/body conflict returns `409`.
23. Client cohort claim is verified, accepted as unverified, or replaced with explicit target provenance.
24. Default runtime response omits full evidence-view detail while audit retains it.
25. OAuth scope denial for each runtime and management operation.
26. Multi-definition registration receipt initializes each exact accepted runtime tuple.
27. Metadata-only bundle update preserves the runtime revision and digest.
28. Approval success for created or semantic changes atomically publishes
    allocated revisions and any captured-baseline activation plans; the
    complete stored receipt appears immediately when no activation is required
    or after every required activation succeeds.
29. Approval rejection, expiration, missing request, digest conflict, idempotent replay, and opposite concurrent action.
30. Expired approval resubmission with the same deterministic apply key creates one linked replacement request after revalidation.
31. Approval review exposes old/new digests, canonical semantic diff, immutable snapshot, and persisted actor/comment metadata.
32. Omitted lineage for new/known keys resolves correctly; unknown or cross-key/scope supplied lineage is rejected.
33. Value/type mismatch, non-finite number, empty/out-of-range confidence, and invalid exposure union are rejected by schemas.
34. Every server `200` contains required definition identity with `integrity: "verified"`.
35. Stable invalid-definition, policy, objective, strategy, signal, authentication, and retry errors.
36. Concurrent same-fingerprint decide requests converge; tracing-only correlation changes do not alter the fingerprint.
37. Decide idempotency TTL expiry permits a new request after 24 hours.
38. Liveness success, global readiness success, evidence degradation, and required global-dependency failure.
39. Evidence-required request behavior does not mutate global readiness classification.
40. Ineligible transport/status failures never produce client fallback.
41. Missing or mismatched local call-site binding forbids both remote decide and availability fallback.
42. `required-evidence-unavailable` is always ineligible for SDK-local fallback; policy may permit an audited server fallback instead.
43. Approval snapshot emits quoted `ETag` and RFC 9530 `Content-Digest` over the same canonical bytes.
44. Code-first and canonical bundle Tetris definitions normalize to the same bytes and `contractDigest`.
45. `definition-not-ready`, `decision-service-not-ready`, and `invalid-decision-state` never produce server or SDK fallback.
46. `service-unavailable` is fallback-eligible only for genuine transient data-plane availability after required readiness passed.
47. Boolean, string, event, or derived metrics referenced by a numeric rule fail bundle validation before approval.
48. Both-present, neither-present, or mismatched state authority payloads fail readiness before lookup.
49. Ready, approval-pending, activation-pending, retryable activation failure,
    permanent activation failure, and rejection use the documented apply
    result variants and HTTP statuses.
50. Exact reapply after permanent activation failure converges on one linked
    authority-reauthorization approval, retains successful partial activations,
    and publishes no additional semantic revision.
51. Every server decision audit carries either explicit no-authority fallback
    or complete selected-authority lineage, with `strategyId` required only for
    numeric-rule authority.

## Contract decision log

All Phase 1 product decisions A1-A12 and their executable OpenAPI, JSON Schema,
fixtures, conformance tests, and mock projection are accepted as the baseline.

### A1. Runtime identity minimum

**Status:** Accepted

**Options**

1. Require only `contractDigest`; treat revision and bundle/build identity as diagnostics.
2. Require `definitionId + revision + contractDigest`.
3. Require the full current `ContractIdentity`.

**Decision:** option 2. Require `definitionId + revision + contractDigest`. `definitionId` is the opaque registry lineage ID; the complete tuple is the immutable runtime identity. `decisionKey` remains the stable developer-facing lookup key. This verifies content without coupling every runtime request to a whole bundle or deployment.

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
2. Apply returns `requires-approval` for new definitions and semantic changes,
   persists the approval request and any proposed lineage identity, and makes
   no accepted runtime publication until explicit approval.
3. The client must provide a new explicit definition ID before apply.

**Decision:** option 2. Apply returns `requires-approval` for new definitions
and semantic changes and makes no runtime publication. It may reserve a new
opaque lineage ID in the immutable approval request. Explicit approval
allocates and publishes new runtime revisions under the affected definition
lineages and durably creates any required activation plans. With no initial
authority, approved publication produces the ready registration receipt;
otherwise every required activation must succeed first. An
authority-reauthorization successor reuses the existing revision.

**Consequence:** under MVP startup registration, every non-ready apply result
rejects initialization. Startup retries or restarts until it receives the
ready binding for the same canonical bundle.

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

## Accepted freeze criteria

The accepted baseline satisfies:

- A1-A12 are represented consistently in the executable artifacts.
- Runtime and management OpenAPI documents validate.
- The bundle JSON Schema validates canonical examples.
- Every required golden scenario has an expected request/result.
- SDK and service can generate or consume types without handwritten wire DTO divergence.
- Basic and advanced SDK authoring forms generate the same canonical definition fixture.
- Contract changes have an explicit compatibility and versioning process.
