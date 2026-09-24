# API contract baseline

Status: implemented manifest-first/runtime baseline. The
[OpenAPI, schemas, fixtures and conformance gate](../../contracts/README.md)
are authoritative for executable shapes. Architecture names logical
services/stores; it does not invent replacement wire fields before migration.

## Scope and ownership

Keep decisions compact, deterministic, typed, constraint-checked and durably
recorded. Separate publication, execution and native telemetry intake.
Confirmation is an explicit operation after application, not sampled evidence.

| Boundary | Current behavior |
| --- | --- |
| Contract Service / control-plane host | Manifest validate/apply, exact approval and approved-definition receipts |
| Decision Service / data-plane host | Exact-identity decide, confirmation and health |
| OTel Ingestion / data-plane adapter | Scoped binary OTLP and durable latest-scalar input materialization |
| Application | Deployment, applying values, OTel instrumentation/export/Collector and sampling |

Manifest v2 replaces producer schemas, inline authoring, extraction and
signal-reference input arrays without adapters/migrations. #49 aligns
executable server/record/constraint terminology; #40 adds initial authority
and activation-ready receipts; #41 verifies final bundle-approved Tetris.
Current trusted bootstrap supplies existing state, not another public format.
Proposal generation, rolling queries, rollout/override/rollback, batch decide
and public record-query APIs remain outside this slice.

## Manifest and identity

One JSON manifest maps keys to result/default, context, targeting, required
request/evidence inputs, native bindings, intent and explicit current `policy`
constraint data. Empty maps and optional context requiredness normalize without
inventing input values.

```text
contractDigest = sha256(RFC8785({ key, contract: normalizedDefinition }))
```

Source semantics, meaning, units, targeting, freshness, sampling and
attribution are semantic. Owner/build/source metadata is publication
provenance; runtime values are not definition identity.

Contract Service allocates opaque lineage/revision. The full tuple is
`{ definitionId, revision, contractDigest }`, never implicit latest selection.
Compiler output is a normalized bundle and typed catalog. Trusted
`@flaggo/sdk/management` publication is separate from synchronous runtime
initialization, which verifies scope, bundle digest, key set and exact
definition digests against the approved receipt.

Required input bindings must use attribution `none`; confirmed-exposure
bindings are supported for outcomes/objectives, not a circular prerequisite
for the first decision.

## API conventions

REST uses camelCase, kebab-case enums, opaque IDs, `application/json` results
and RFC 9457 `application/problem+json` errors. Programs use status, stable
`code`, issue fields and JSON Pointers rather than parsing prose.
Strict JSON rejects duplicates, closed-schema unknown fields, wrong primitives
and non-finite/rounding-unsafe numbers. Null is not an omitted input/default.

OTLP instead uses native `application/x-protobuf` export requests/responses
and `google.rpc.Status` errors, not JSON Problem Details.

### Correlation and retries

`X-Flaggo-Correlation-Id` is an echoed/generated header, not caller identity.
It and tracing metadata do not change decide's fingerprint.

An optional `Idempotency-Key` names retries within authenticated
tenant/application/environment and operation. Same key and canonical caller
request return the retained result; changed content conflicts. Evidence
generation/evaluation time is not part of the fingerprint.
Concurrent equal calls converge; bounded waiting can return
`idempotency-in-progress` with a retry hint.

The current in-memory store retains successes and deterministic 4xx for 24
hours. A 5xx releases the claim after concurrent followers converge so
dependencies can recover. Expiry permits a new request; this is not durable
cross-process deduplication.

## Runtime API

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

Decide carries `expectedContract`, `client`, `runtimeContext`, an optional
runtime target and a primitive `inputs` object. Only request-owned operands
are supplied:

```json
{
  "boardPressure": 0.82,
  "currentLevel": 3,
  "recentPlacementTimeMs": 1420,
  "recoveryFailures": 2
}
```

Caller context/target is validated before evidence resolution. No static
definition, binding selector, snapshot, constraint override or tenant claim
belongs in the request. The verified scope comes from the host.

Decision Service resolves one immutable evidence generation, then passes only
definition, numeric rule and resolved primitive map to the executor.
Optional constraint-quality evidence is a separate port; request-only
decisions with no quality constraint use neither evidence reader.

Success includes verified identity, value/type, targets, current `policy`
evaluation, fallback, `decisionId`/`auditId`, reason and exposure directive.
These current wire names do not create standalone Policy/Audit components.
Deterministic rules return null learned confidence. The default SDK receipt
is smaller than its detailed result.

| Outcome | Behavior |
| --- | --- |
| Approved authority or governed fallback | Recorded HTTP 200, not an infrastructure failure |
| Missing/unknown/conflicting/retired identity | Explicit contract error, never SDK fallback |
| Old input array or duplicate JSON property | Strict 400 wire rejection |
| Invalid caller operand/source override | 422 `invalid-inference-input` with input-specific issue/path |
| Unusable required evidence operand | 503 `required-evidence-unavailable`, `clientFallback.eligible: false` |
| Required persistence/state integrity failure | Explicit error, not missing-state fallback |
| Recognized eligible outage | Local default only when explicitly enabled and narrowly classified |

SDK fallback has no server decision, successful constraint evaluation, record
or exposure. A retry hint does not grant fallback eligibility.

Confirmation uses the original opaque capability and authorized scope, accepts
no replacement inputs and preserves one exposure on exact replay.
Durable exposure append precedes the final confirmation commit. A pending
receipt or append alone is not confirmed. Capabilities never enter record
output.

`confirmedExposureAttributes` supplies ordinary attributes after confirmation.
Attribution separately verifies scope, exact definition and resolved target.
Current confirmation lookup is in-memory: new references to lost confirmations
fail closed after restart, while retained validated frames and their exact
redelivery retain provenance.

## Management API

```http
POST /v1/definition-bundles:validate
POST /v1/definition-bundles:apply
GET  /v1/approvals/{approvalRequestId}
GET  /v1/approvals/{approvalRequestId}/snapshot
POST /v1/approvals/{approvalRequestId}:approve
POST /v1/approvals/{approvalRequestId}:reject
```

Validate is read-only and returns validity, compatibility and identities.
Apply uses deterministic scope/bundle idempotency. New or semantic changes
return 202 `requires-approval`, not an accepted runtime binding.
Exact-snapshot approval is authenticated and checks captured baselines.
Opposite terminal operations, expiry, mismatched digest and key/body conflicts
are explicit errors.

Accepted apply returns 200 `status: "approved"` and exact
`acceptedDefinitions`. It does not claim initial-authority activation or
ready-after-activation. The SDK helper never auto-approves; trusted local
example tooling explicitly approves. Runtime initialization makes no
management request.

Invalid source/type/target/attribution combinations, circular exposure inputs,
unknown authored identities and unresolved references fail validation; apply
cannot publish them. Metadata edits preserve semantic identity, not approval
for semantic edits. Snapshots expose canonical bytes with a quoted ETag and
RFC 9530 Content-Digest. Expired revalidation converges on one linked request.

## OTLP boundary

```text
/otlp/{appId}/{environment}/v1/metrics
/otlp/{appId}/{environment}/v1/traces
/otlp/{appId}/{environment}/v1/logs
```

Separate `polari.telemetry:ingest` permission and authenticated
tenant/application/environment partition inputs. Resource attributes cannot
authorize access. Native binary bodies support uncompressed/identity and gzip;
encoded/decompressed bytes, decoded work, bindings, frames and snapshots are
bounded. Accepted observations are acknowledged only after durable publication.
Partial success rejects a native record only when every matching binding
rejects it. Capacity/unavailability returns 429/503 with retry hints.

Bindings support latest Gauge, span duration/attribute, named span-event, and
structured-log attribute/body scalar. Freshness retains source nanoseconds.
Replay cannot refresh age; conflicts or multiple fresh Gauge streams are
ambiguous. No text-to-JSON parsing, counter/histogram conversion, rate DSL,
trace joins, population claims, exemplar attribution or sampling correction.
See [OTel Ingestion](otel-ingestion/README.md) for exact limits/responses and
[Collector integration](../../examples/otel-evidence/README.md).

## Authentication, readiness and conformance

Production uses OAuth/OIDC, verified scope and operation-specific permissions.
Explicit development bypass cannot run outside Development. Browser code has
neither management nor ingest credentials.

Health exposes coarse stable states, never paths, credentials or exceptions.
Required dependency failure is not-ready; optional quality evidence can be
degraded. A per-request input failure does not rewrite global readiness or make
live-input decisions depend on a Collector.

The [fixture manifest](../../contracts/conformance/fixture-manifest-v1.json)
owns 43 golden scenario IDs covering result/identity validation, fallback,
source errors, confirmation, publication/approval, exact replay/concurrency,
scope denial for every secured operation, retention, health and snapshot
integrity. The exhaustive service runner owns each case. Additional generated
typing, native projection/persistence/isolation and real Collector tests
validate boundaries beyond the fixture count.

Wire changes update schemas, OpenAPI, fixtures, consumers and owning documents
together. Future authority, temporal and proposal work consumes these
boundaries through its accepted design, not dormant compatibility paths.
