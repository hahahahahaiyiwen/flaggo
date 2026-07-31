# Phase 1 API Contract Proposal

Status: **Accepted Phase 1 executable contract baseline**

This proposal is the accepted prose design source for Phase 1. The executable
baseline under [`contracts/`](../../contracts/README.md) encodes it and passes
the conformance gate. Later behavior changes require an explicit contract
revision with aligned schemas, OpenAPI, fixtures, and compatibility notes.

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
- SDK duration shorthand such as `"20s"` is normalized into canonical domain fields such as `{ "seconds": 20 }` before transport or hashing.

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

- Success payloads use `application/json`.
- Errors use `application/problem+json` following RFC 9457 Problem Details.
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
  }
}
```

The HTTP adapter combines the route key and body into the internal canonical `DecideRequest` and maps `X-Flaggo-Correlation-Id` into its tracing metadata.

Request invariants:

- `expectedContract` is required and contains `definitionId + revision + contractDigest`.
- Optional `Idempotency-Key` controls retry deduplication; the correlation header remains tracing metadata and is not uniqueness identity.
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
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
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
  "reason": "Approved strategy slowed the drop interval within policy bounds.",
  "auditId": "audit-789"
}
```

Server response invariants:

- A successful wire response always has `decisionId`, `auditId`, and `fallback.source = "server"`.
- A successful wire response always repeats the exact accepted `definitionId + revision + contractDigest` and has `definitionStatus.integrity = "verified"`.
- `runtimeTarget` and `controlTarget` are optional. Global or otherwise targetless decisions omit them and return an empty provenance list with a resolution chain ending at `global`.
- `valueType` and `value` form a discriminated union: boolean with boolean, number with finite JSON number, and string with string.
- The initial response never has `exposureId`.
- `confidence` is `null` when `decisionFallbackUsed` is true and may also be null for a non-evidence-based `active-value`.
- `confidence` is required for `strategy` and `experiment` modes and whenever the reason claims evidence-backed adaptation. Resolution fallback therefore retains confidence when a broader target produced an approved evidence-backed decision.
- A non-null confidence object requires `evidenceQuality`; an empty object is invalid. Every confidence field is in the inclusive range `[0, 1]`, and higher `modelUncertainty` means less certainty.
- Exposure metadata is a union. `confirmationRequired: true` requires `confirmToken`; `confirmationRequired: false` forbids it.
- Set-like arrays are emitted in canonical order; semantically ordered arrays retain their defined order.
- The response contains structured reason codes in policy/fallback fields. Human-readable `reason` is explanatory and must not be used for program logic.
- A blocked policy never approves its candidate. When policy supplies the governed fallback value, the `200` response uses `decisionMode: "fallback"`, `decisionFallbackUsed: true`, and may report `policy.result: "blocked"`.
- The default response returns compact confidence and target-resolution provenance. Full evidence-view details remain in the audit record.

The SDK may project this response into `DecisionReceipt<T>` or a detailed result. If the data plane is unavailable and application configuration explicitly enables availability fallback, the SDK creates a distinct client-fallback result with no server `decisionId`, `auditId`, `policy`, `definitionStatus`, or exposure confirmation metadata. It must never do this for a 4xx contract/configuration response.

### SDK availability-fallback classifier

Availability fallback is disabled by default. When enabled, it is eligible only after the SDK exhausts its retry policy for:

- DNS failure, connection refusal/reset, or connection/read timeout before a complete HTTP response,
- intermediary HTTP `502` or `504` responses,
- a valid Flaggo `5xx` Problem Details response only when `clientFallback.eligible` is explicitly `true`.

It is forbidden for TLS/certificate validation failures, proxy/authentication configuration failures, cancellation requested by application code, malformed responses, every HTTP `503` without explicit `clientFallback.eligible: true`, HTTP `4xx` including `408` and `429`, HTTP `500`, `501`, or `505`, every contract/configuration error, and every valid Flaggo Problem Details response whose eligibility is false or absent.

`required-evidence-unavailable` is forbidden by default even though its status is `503`. A definition policy must separately set `clientFallback.requiredEvidenceUnavailable = "allow"` before the server may return:

```json
{
  "type": "https://flaggo.dev/problems/required-evidence-unavailable",
  "title": "Required evidence unavailable",
  "status": 503,
  "code": "required-evidence-unavailable",
  "clientFallback": {
    "eligible": true,
    "reason": "policy-permitted-required-evidence-unavailable"
  }
}
```

Without that permission, the same error carries `eligible: false`; the SDK surfaces it after retries and must not return a local value. `service-unavailable` sets `eligible: true` when the server can emit Problem Details. The SDK's own availability-fallback configuration is still required in every eligible case, so server permission cannot enable fallback by itself.

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
3. accepts only an approved registration receipt,
4. initializes the data-plane binding from the receipt,
5. rejects client initialization with a typed error when validation fails or approval is required; no data-plane client is created.

The startup caller requires management authorization. A browser or other untrusted runtime must not contain a long-lived control-plane credential.

### Approve a semantic revision

When apply detects a semantic change under an existing decision key, it returns `status: "requires-approval"` plus an `approvalRequestId` and performs no registry mutation.

Apply returns `200` with `RegistrationReceipt` when immediately approved, `202` with the following result when approval is pending, and `422 invalid-bundle` when validation rejects the write:

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

Every approval resource variant requires `approvalRequestId`, `application`, `environment`, `bundleDigest`, `createdAt`, `expiresAt`, `changes`, and `snapshotUrl`. `changes` is non-empty, contains at least one `semantic-change`, and every semantic change carries a non-empty canonical `semanticDiff`. A renewed request also carries `supersedesApprovalRequestId`. Its status-specific fields form a discriminated union:

- `pending` has no receipt or terminal decision.
- `approved` requires `decidedAt`, the complete `RegistrationReceipt`, and approval metadata containing the server-derived actor plus optional persisted comment.
- `rejected` requires `decidedAt` and rejection metadata containing the server-derived actor, `reasonCode`, and optional persisted comment.
- `expired` requires `expiredAt` and has no receipt. Expiration is terminal.

`GET` and a successful terminal action return `200` with that union. Approve/reject never return an approval-shaped success before the compare-and-swap transition has committed. The approved variant embeds the exact receipt shape from bundle apply; clients initialize runtime bindings only from that receipt.

`GET .../{approvalRequestId}/bundle` returns the exact canonical `DecisionDefinitionBundle` snapshot used to calculate `bundleDigest`. It is immutable across every approval state and retained with the approval audit record. `changes` contains the previous accepted tuple, proposed lineage/digest, and a canonical semantic diff sorted by JSON Pointer path. The diff uses `add`, `remove`, and `replace` operations over compatibility-critical canonical definition content; it excludes build metadata and other fields outside `contractDigest`.

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
- Approve verifies `expectedBundleDigest`, then atomically transitions `pending -> approved`, creates the new runtime revisions, applies the whole pending bundle, and stores the receipt.
- Reject verifies `expectedBundleDigest`, then atomically transitions `pending -> rejected` without registry mutation.
- Repeating the same terminal action with the same digest is idempotent and returns the stored result. No separate idempotency key is required.
- Concurrent terminal actions use one compare-and-swap transition. One wins; the same action converges on its result, while the opposite action returns `409 approval-terminal-conflict`.
- An unknown request returns `404 approval-not-found`. `GET` returns an expired resource as `200`; approve/reject against it return `410 approval-expired`. A digest mismatch returns `409 approval-bundle-conflict`.
- Terminal states never transition again. Approval/rejection authorization and application/environment scope are checked on every action.
- A startup retry using the same canonical bundle and deterministic apply idempotency key returns the approved receipt after approval; before approval it returns the same `requires-approval` result.
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

`requires-approval` is also strict: it requires at least one `semantic-change`, every semantic change requires a non-empty `semanticDiff`, and `issues` may contain warnings only. Approval resources preserve that semantic-change invariant in every lifecycle state.

Each definition entry is also a discriminated union keyed by `valueType`. Boolean, number, and string entries require matching action-space defaults and fallback value types. Numeric bounds must ascend, defaults and fallbacks must be in range, `step` must be positive, and numeric defaults/fallbacks must align to it. String defaults and fallbacks must belong to `allowedValues` when that set is present.

Optional `definitionId` rules:

- Omission is required for a new decision key. Validation computes its proposed `contractDigest`; apply assigns the new opaque lineage ID and initial revision.
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
  "status": "approved",
  "compatibility": "identical",
  "acceptedDefinitions": {
    "tetris.dropInterval": {
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
      "contractDigest": "sha256:contract..."
    }
  },
  "changes": [],
  "issues": []
}
```

Apply invariants:

- The bundle is the management write unit.
- `acceptedDefinitions` is the only source for initializing per-decision runtime bindings; clients must not construct identities from the bundle digest or decision key.
- A repeated request with the same idempotency key and canonical body returns the original result while its approval is pending or terminally approved/rejected. Expiration releases that apply attempt for the renewal behavior above.
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
- The Phase 1 required checks are contract registry, decision state, policy evaluation, and durable audit. Evidence is globally optional: its loss always yields `200 degraded` readiness and never dynamically changes the check's `required` flag.
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
| Invalid runtime context | `422 invalid-runtime-context` Problem Details |
| Invalid declared input/value | `422 invalid-inference-input` Problem Details |
| Missing or invalid credentials | `401 authentication-required` Problem Details |
| Missing operation scope | `403 insufficient-scope` Problem Details |
| Credential/body application or environment mismatch | `403 scope-mismatch` Problem Details |
| Decide key reused with another fingerprint | `409 idempotency-conflict` Problem Details |
| Matching decide request still executing after wait budget | `409 idempotency-in-progress` Problem Details plus `Retry-After` |
| Rate limit | `429 rate-limited` Problem Details |
| Definition requires evidence that is currently unavailable and policy forbids governed fallback | `503 required-evidence-unavailable`; client fallback is forbidden unless separately policy-authorized in the Problem Details extension |
| Runtime unavailable before an audited decision exists | `503 service-unavailable` Problem Details; SDK may use explicitly configured availability fallback |

The key distinction is whether the server completed an audited evaluation of a valid registered definition. A completed governed fallback is a decision result. Contract/configuration rejection is an actionable error and forbids local fallback. Transport or data-plane availability failure may use explicitly configured local fallback.

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
16. Semantic bundle change returns `202 requires-approval` with no mutation.
17. Idempotent bundle apply and key/body conflict.
18. Eligible `503 service-unavailable` SDK-local fallback after retry exhaustion when explicitly enabled.
19. Concurrent startup registration of the same bundle returns one accepted identity.
20. Invalid or approval-pending startup registration does not initialize the data-plane client.
21. Optional decide idempotency replay returns the original decision.
22. Decide idempotency key/body conflict returns `409`.
23. Client cohort claim is verified, accepted as unverified, or replaced with explicit target provenance.
24. Default runtime response omits full evidence-view detail while audit retains it.
25. OAuth scope denial for each runtime and management operation.
26. Multi-definition registration receipt initializes each exact accepted runtime tuple.
27. Metadata-only bundle update preserves the runtime revision and digest.
28. Approval success atomically creates revisions and returns the complete stored receipt.
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
42. `required-evidence-unavailable` defaults to client fallback forbidden; explicit effective policy allow plus SDK configuration makes it eligible.
43. Approval snapshot emits quoted `ETag` and RFC 9530 `Content-Digest` over the same canonical bytes.

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
2. Apply returns `requires-approval` and makes no mutation until an explicit approval operation.
3. The client must provide a new explicit definition ID before apply.

**Decision:** option 2. Apply returns `requires-approval` and makes no mutation. The approval resource records review state; explicit approval atomically creates new opaque runtime revisions under the affected definition lineages, applies the pending canonical bundle, and produces the registration receipt.

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

## Accepted freeze criteria

The accepted baseline satisfies:

- A1-A12 are represented consistently in the executable artifacts.
- Runtime and management OpenAPI documents validate.
- The bundle JSON Schema validates canonical examples.
- Every required golden scenario has an expected request/result.
- SDK and service can generate or consume types without handwritten wire DTO divergence.
- Basic and advanced SDK authoring forms generate the same canonical definition fixture.
- Contract changes have an explicit compatibility and versioning process.
