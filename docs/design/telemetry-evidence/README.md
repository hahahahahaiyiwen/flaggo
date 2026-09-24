# Telemetry and evidence design

## Boundary

The application owns instrumentation and collection. Flaggo receives native
OTLP, interprets approved decision-local bindings, and materializes bounded
scalar input evidence outside decision evaluation. It does not define producer
schemas, replace application exporters, retain raw telemetry, or infer model
confidence from observations.

| Owner | Contract |
| --- | --- |
| Data-plane adapter | OTLP Protobuf decoding, HTTP/authentication, byte/work limits, protocol responses |
| Registry | Typed approved binding projection under verified scope |
| Evidence | `IInputTelemetrySink`, `IInputEvidenceReader`, `IInputEvidenceSnapshotStore`, latest-value/freshness semantics |
| State | `IConfirmedExposureReader` for committed attribution |
| Reasoning | Required typed input resolution from one pinned generation |
| Audit | Caller/resolved vectors and provenance, copied into confirmed exposure |

`IEvidenceProvider` remains a separate optional policy-quality port. A
request-only rule with no evidence-dependent policy reads neither provider.
The executor sees only resolved primitive values, never either snapshot type.

## OTLP/HTTP transport

```text
POST /otlp/{appId}/{environment}/v1/metrics
POST /otlp/{appId}/{environment}/v1/traces
POST /otlp/{appId}/{environment}/v1/logs
```

Only binary `application/x-protobuf` with uncompressed/identity or gzip bodies
is supported. JSON OTLP, gRPC, and profiles are not supported.
Official protocol models are generated from pinned upstream
`opentelemetry-proto` v1.11.0; provenance/license are beside the host proto files.

`polari.telemetry:ingest` is separate from decide/confirm authorization.
Authenticated `polari_tenant_id`, `polari_app_id`, and `polari_environment`
establish scope. Route app/environment must be authorized; telemetry resource
attributes cannot broaden it. Trusted development uses the distinct
`Flaggo-Local-Telemetry` authorization header; never use that bypass publicly.

| HTTP outcome | Native payload and meaning |
| --- | --- |
| 200 | Signal-specific `Export*ServiceResponse`, only after durable acceptance |
| 200 partial success | Rejected native record count plus bounded binding diagnostics; not a retry request |
| 400 | Malformed Protobuf/gzip/native structure |
| 401/403 | Authentication or authorized-scope failure |
| 413 | Encoded/decompressed bytes or decoded work limit exceeded |
| 415 | Unsupported media type or content encoding |
| 429/503 | Capacity or durable-materialization unavailable; `Retry-After: 1` |

Errors use Protobuf `google.rpc.Status`, not Flaggo JSON Problem Details.
Unknown optional Protobuf fields retain native protocol behavior.
Unmatched records are discarded with counters. If several bindings match a
record, reject the OTLP record only when none accept it; report individual
binding failures separately. Diagnostics are bounded to 2,048 response
characters. Counters distinguish accepted/rejected/unmatched by signal kind.

## Configuration

Host configuration uses `Flaggo:Telemetry:*` (double underscores in environment
variables). All limits must be positive integers.

| Setting | Default |
| --- | --- |
| `MaximumBodyBytes` | 4 MiB, independently bounding encoded and decompressed bodies |
| `MaximumRecords` | 1,000 decoded records/work items |
| `MaximumBindings` | 1,000 active projected bindings |
| `MaximumFrames` | 10,000 retained target/stream frames |
| `MaximumSnapshotBytes` | 16 MiB |
| `CommitDescriptorPath` | `data/telemetry/inputs.commit.json`, relative to host content root |

The complete local
[Collector configuration](../../../examples/otel-evidence/collector.yaml)
uses stock `otelcol` 0.161.0 and `otlp_http/flaggo`, encoding `proto`,
compression `gzip`, and scoped base URL ending in `/otlp/<app>/<environment>`.
The exporter appends `/v1/<signal>`. Its receiver binds loopback, and independent
existing-backend pipelines remain configured. Credentials belong to Collector
environment/configuration, not manifests, catalogs, or browsers.

## Projection and freshness

See [Evidence](../../architecture/EVIDENCE.md) for supported Gauge, span
duration/attribute, named span-event, and log attribute/structured-body
projections. Each binding declares meaning, primitive schema, selectors,
target, latest projection, freshness, observed-only sampling, and attribution.

Preserve source nanoseconds as decimal strings. Freshness is inclusive and
based on source time, not ingestion/retry/file time. Missing/future/stale
observations are unusable. Exact replay and older delivery cannot refresh a
value. Same-time conflicts or multiple fresh Gauge streams are ambiguous.
New invalid identifiable observations invalidate the last good value.

No rolling aggregation, counter conversion, JSON-string parsing, implicit
zero, complete-population claim, or learned confidence is produced.

## Durable publication and recovery

Keys include authenticated tenant/application/environment, exact definition
identity, binding, and resolved target; Gauge frames also retain stream
identity. New revisions start with their own frames.

Ingestion serializes publication through a single writer lease. It pins
registered bindings, validates a bounded candidate, publishes through
`CommittedFileSnapshotWriter`, and only then swaps the immutable read view.
Readers capture one view and evaluation time for all requested inputs.
Expired frames may be reclaimed on ingestion, but fresh capacity is never
silently evicted.

Restart verifies descriptor/path/length/digest/version and the entire snapshot.
Corruption or uncertain publication invalidates reads until a verified reload.
Repair/restore the committed artifacts and restart to reload; do not erase an
existing corrupt store to manufacture an empty success. Competing writers and
oversized snapshots fail explicitly.

## Attribution and failure

Only committed exposures can satisfy confirmed-attribution span/log bindings.
Lookup checks tenant/application/environment, exact definition, and the
resolved target captured with the decision. Gauge exemplars, sampled trace
context, pending receipts, and a bare decision ID do not prove application.

Missing/stale/ambiguous/future/invalid required input evidence yields
`503 required-evidence-unavailable`, always SDK-fallback-ineligible.
Policy-quality evidence retains its distinct explicit policy path. Neither
path grants authority or changes the numeric executor into a telemetry query.

## Validation and non-goals

Domain/HTTP coverage includes native signal projections, strict scope and
source matching, freshness boundaries, replay, ambiguity, invalidation,
durability/restart/corruption, generation pinning, capacity, protobuf/gzip,
authorization, partial success, audit, and confirmation.
The [real SDK/Collector example](../../../examples/otel-evidence/README.md)
proves the complete flow without a cloud account.

No warehouse, long-term retention, query DSL, high-cardinality optimization,
sampling correction, model training, or proposal generation is introduced.
