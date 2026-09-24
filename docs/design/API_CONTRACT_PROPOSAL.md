# API contract baseline

Status: accepted runtime baseline with the #44 manifest-first replacement.
The [OpenAPI, schemas, fixtures, and conformance gate](../../contracts/README.md)
are authoritative for executable behavior. This document explains their
boundaries; it does not define a parallel producer or authority contract.

## Goals and scope

Keep runtime decisions compact, deterministic, typed, policy-gated, and durably
audited. Separate authenticated contract publication, runtime execution, and
telemetry ingestion. A returned value is not an exposure; confirmation occurs
only after application/rendering.

Manifest `flaggo.decision-definition-bundle/v2` replaces producer declarations,
inline definition authoring, extraction, redundant defaults, and signal-ref
input arrays. There is no old-format adapter or migration. The REST operation
names remain versioned independently of the manifest format.

| Boundary | Implemented behavior |
| --- | --- |
| Control plane | Manifest validate/apply, immutable exact-snapshot approval, and approved-definition receipts |
| Runtime data plane | Exact-identity decide, explicit exposure confirmation, and health |
| Telemetry data plane | Scoped binary OTLP/HTTP metrics, traces and logs; durable latest-value input materialization |
| Application | Owns deployment, applying values, OTel instrumentation/providers/Collector, and sampling |

#40 extends this manifest with initial authority and activation-ready receipts;
#41 verifies the final bundle-approved Tetris path. Current local examples
provision existing governed state in trusted tooling. Those fixtures do not
create another public authoring format or claim completed authority readiness.
Proposal generation, arbitrary telemetry queries, rollout, override, rollback,
batch decisions, and public audit queries remain outside this slice.

## Manifest and identity

One JSON manifest maps keys to `result`, `context`, `targeting`, `inputs`,
`evidence`, optional `intent`, and explicit `policy`. One result contract owns
the safe default. Every declared operand is required and is either
request-owned or references a decision-local evidence binding.

Normalization fills omitted empty maps and context `required: false`, without
inventing operand values. Semantic identity is:

```text
contractDigest = sha256(RFC8785({ key, contract: normalizedDefinition }))
```

Definition owner metadata is excluded from semantic identity. Bundle metadata
participates in bundle identity. Source selection, meaning, units, freshness,
sampling acceptance and attribution are semantic. Runtime values are not.

The registry issues opaque `definitionId` and `revision`. The accepted runtime
identity is the complete `{ definitionId, revision, contractDigest }` tuple.
Metadata-only publication preserves it; an approved semantic change produces
a new immutable revision. The client never selects implicit "latest."

The compiler generates a normalized bundle and typed runtime catalog. Trusted
code publishes through `@flaggo/sdk/management`; the runtime client verifies
application/environment, bundle digest, exact decision keys and every accepted
definition digest against the catalog before capturing an immutable binding.

See [decision definition](../architecture/DECISION_DEFINITION.md) and
[client design](client-library/README.md) for the authored contract.

## API conventions

REST bodies use camelCase, enum values use kebab-case, and IDs are opaque.
Management and runtime JSON reject duplicate properties, unknown members where
their schema is closed, wrong primitives, and non-finite or rounding-unsafe
numbers. Optional fields follow the schema; explicit null is not an omitted
operand or an instruction to infer a default.

REST results use `application/json`; failures use RFC 9457
`application/problem+json`. Programs branch on `code`, HTTP status, and
structured issue fields, not prose. Issues identify a JSON Pointer and, where
applicable, `decisionKey`/`inputKey`.

OTLP uses `application/x-protobuf` for native requests, acknowledgements and
`google.rpc.Status` errors. It is not a JSON Problem Details transport.

### Correlation and retries

`X-Flaggo-Correlation-Id` is a header, not caller JSON identity. The server
echoes it or supplies one. It and tracing metadata do not change the canonical
decide fingerprint.

An optional `Idempotency-Key` names decide retries within the authenticated
tenant/application/environment and operation. Same key plus the same
RFC 8785 caller request returns the retained decision; changed content
conflicts. Successful replay retains the original resolved inputs and
provenance rather than reading newer evidence. Concurrent equal requests
coalesce; a bounded wait can return `idempotency-in-progress` with a retry hint.

The current in-memory store retains success and deterministic 4xx outcomes for
24 hours. Every 5xx releases the claim after concurrent followers converge, so
recovered dependencies can be retried. Expired keys are new requests; this is
not durable deduplication across process restart.

## Runtime API

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

The route supplies the decision key. Decide carries `expectedContract`,
`client`, optional runtime target/context, and a plain primitive `inputs` map:

```json
{
  "boardPressure": 0.82,
  "currentLevel": 3,
  "recentPlacementTimeMs": 1420,
  "recoveryFailures": 2
}
```

Only request-owned operands cross this field. Unknown keys, missing required
values, type/range errors, and evidence-owned overrides are explicit failures.
Context and target claims are validated before source resolution. The caller
cannot submit a policy, static definition, binding selector, evidence snapshot,
or tenant identity.

The host passes verified scope to a reasoning-owned resolver. It reads one
materialized generation/evaluation time, then passes only the definition,
numeric rule and resolved primitive map to the bounded executor. Policy-quality
evidence is a separate optional dependency. A request-only decision without
evidence-dependent policy reads neither evidence port.

Every server success has exact definition identity, policy/fallback provenance,
decision/audit IDs and an explicit exposure union. Deterministic numeric rules
have `confidence: null`; received observations are not learned confidence.
The minimal SDK receipt and detailed result remain separate projections.

| Outcome | Behavior |
| --- | --- |
| Approved authority or governed fallback | Audited HTTP 200; fallback is not an infrastructure error |
| Missing/unknown/conflicting/retired contract | Explicit contract error; never SDK fallback |
| Legacy input array or duplicate JSON properties | 400 strict wire rejection |
| Invalid caller operand or source override | 422 `invalid-inference-input` with input-specific issue/path |
| Missing/stale/future/ambiguous/invalid required input evidence | 503 `required-evidence-unavailable`, `clientFallback.eligible: false` |
| Required readiness or state-integrity failure | Explicit error, not missing-state fallback |
| Recognized eligible data-plane outage | SDK-local default only when explicitly enabled and narrowly classified |

SDK fallback has no server decision ID, audit, policy success or exposure.
An HTTP retry hint alone never grants fallback eligibility.

Confirmation requires the original opaque capability and authorized scope,
accepts no replacement inputs, and is idempotent for the same observation.
Decision-time inputs/provenance are captured immutably. Durable exposure audit
precedes the final confirmation commit; audit append alone is not proof of
completed confirmation. The token is not audit data.

`confirmedExposureAttributes` is a pure helper for ordinary application
telemetry after confirmation. Attributed evidence additionally verifies exact
definition, scope, and resolved target. The current confirmation store is
in-memory: new late observations cannot reconstruct lost confirmations after
restart; already verified durable frames retain their provenance.

## Management API

```http
POST /v1/definition-bundles:validate
POST /v1/definition-bundles:apply
GET  /v1/approvals/{approvalRequestId}
GET  /v1/approvals/{approvalRequestId}/snapshot
POST /v1/approvals/{approvalRequestId}:approve
POST /v1/approvals/{approvalRequestId}:reject
```

Validate is read-only and returns structured validity, compatibility, and
computed identities. Apply uses a deterministic idempotency key derived from
application/environment/bundle digest. New or semantic definitions require
approval and return 202 `requires-approval`, not an accepted runtime binding.
Approval is an explicit authenticated operation over an immutable exact
snapshot. Opposite terminal actions, expiration, digest mismatch, and body/key
conflicts remain actionable errors.

Accepted apply returns a 200 receipt with `status: "approved"` and the exact
`acceptedDefinitions` map. It does not claim #40's initial-authority activation
or ready-after-activation fields. The SDK publication helper never approves
automatically; trusted local example tooling performs that explicit action.
Runtime initialization itself makes no management request.

Unknown author-supplied lineage, invalid reference/policy/type/targeting, and
unsupported projection semantics fail manifest validation. A well-formed
invalid manifest yields an invalid validation result; apply cannot publish it.
Metadata-only edits do not create a semantic revision or bypass approval for a
semantic edit.

Approval snapshots retain canonical bytes with a quoted ETag and RFC 9530
Content-Digest. Expired resubmission is revalidated and converges on one linked
replacement approval. Exact retry does not invent a new accepted identity.

## OTLP boundary

```text
/otlp/{appId}/{environment}/v1/metrics
/otlp/{appId}/{environment}/v1/traces
/otlp/{appId}/{environment}/v1/logs
```

The separate `polari.telemetry:ingest` permission does not follow from
decide/confirm credentials. Authenticated tenant and authorized route scope
partition frames and runtime reads; resource attributes are metadata only.

Binary Protobuf supports uncompressed/identity and gzip. Decoded/encoded bytes,
record work, active bindings, frames and snapshot bytes are bounded. Accepted
observations are acknowledged only after durable publication. Partial success
counts a record as rejected only when no matching binding accepted it.
Malformed/oversized input yields 400/413; capacity/unavailable storage yields
429/503 with retry hints.

Supported bindings select a latest Gauge scalar, span duration/attribute,
named span-event attribute, or structured log attribute/body scalar.
Freshness uses original nanosecond time. Replay cannot refresh age; conflicting
latest values or multiple fresh Gauge streams are ambiguous. String bodies
are not parsed as JSON. Sampling acceptance is explicitly observed-only.
Counter/histogram conversion, rates, rolling windows, trace joins, population
claims, exemplar attribution and sampling correction are not supported.

See [evidence](../architecture/EVIDENCE.md) and the
[real Collector integration](../../examples/otel-evidence/README.md).

## Authentication and health

Production uses OAuth/OIDC and operation-specific scopes. The host requires
verified tenant/application/environment claims. The Development-only bypass
must be explicitly enabled and cannot run in another environment. Browser
code never receives management or Collector ingest credentials.

Liveness and readiness expose coarse stable states, not paths, credentials or
exception details. Required dependency failure is not-ready; optional
policy-evidence loss can be degraded. A request's evidence failure does not
rewrite global readiness or make request-only decisions depend on a Collector.

## Required golden scenarios

The fixture manifest retains 43 scenario IDs; multiple fixtures may cover one
scenario. The exhaustive service runner owns an executable plan for every case.

1. Active fixed value.
2. Active numeric strategy with null learned confidence.
3. Resolution fallback retaining approved result provenance.
4. Policy decision fallback with null confidence.
5. Unknown decision key.
6. Missing exact identity without local fallback.
7. Unknown expected definition without local fallback.
8. Conflicting contract identity without local fallback.
9. Retired definition without local fallback.
10. Invalid old input shape and strict duplicate-property rejection.
11. Input type/range mismatch.
12. Exposure confirmation and idempotent retry.
13. Invalid exposure capability.
14. Valid manifest with identical compatibility.
15. Invalid manifest with structured issues and no publication.
16. New/semantic definition requires explicit approval.
17. Idempotent apply and key/body conflict.
18. Narrowly eligible availability fallback after configured retries.
19. Concurrent explicit publication converges on one accepted identity.
20. Invalid/pending publication cannot initialize an accepted runtime binding.
21. Decide replay returns the original outcome.
22. Decide key/body conflict.
23. Verified or explicitly replaced target claims and provenance.
24. Compact runtime result; detailed evidence/provenance remains in audit.
25. Scope denial for every secured REST and native OTLP operation.
26. Multi-definition receipt binds each exact key/identity.
27. Metadata-only publication preserves semantic identity.
28. Authenticated approval publishes accepted revisions.
29. Rejection, expiration, absence, digest conflict, and terminal replay.
30. Expired approval resubmission converges on a linked replacement.
31. Approval review, semantic diff, snapshot, actor and comments.
32. Registry-owned lineage; reject unknown or cross-scope supplied identity.
33. Result/type/finite-number/confidence/exposure-union shape failures.
34. Server success has verified exact definition identity.
35. Stable definition, policy, input, authentication and retry failures.
36. Concurrent same-fingerprint convergence independent of tracing metadata.
37. Decide retention expiry permits a new request.
38. Liveness, readiness, optional degradation and required dependency failure.
39. Required-evidence failure does not mutate global readiness.
40. Ineligible failures never produce SDK fallback.
41. Invalid local catalog/receipt binding forbids decide and local fallback.
42. Required-evidence failure never grants SDK-local fallback.
43. Snapshot ETag and Content-Digest cover the same canonical bytes.

Native materialization, generated typing, persisted input generations, tenant
isolation, source ownership, and the real emitter/Collector path have additional
boundary and integration tests. They are not replaced by fixture count alone.

## Contract decision log

The accepted choices are one manifest, exact approved identities, mandatory
policy, compact runtime results, explicit idempotent confirmation, separate
ingest authorization, no hidden registration, and fail-closed source ownership.
Governed fallback is a completed audited result; SDK fallback is a separately
enabled availability behavior. Unknown sampling cannot become confidence or
population evidence.

Future authority/activation, temporal and proposal work must consume these
contracts through its own accepted design, not introduce dormant compatibility
paths here. Any wire change updates schemas, OpenAPI, fixtures, consumers and
these notes together.
