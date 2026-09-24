# OpenTelemetry ingestion design

## Ownership

OTel Ingestion accepts application-owned telemetry and writes interpreted
evidence through Evidence Store ports. Applications own OTel APIs/providers,
sampling, exporters, buffering and Collector routing. Flaggo requires no
replacement producer schema or second client exporter.

The host adapter owns Protobuf/HTTP/authentication and work limits. Contract
Service supplies approved typed bindings. Materialization owns scalar
projection, freshness and durable generations. Decision Service resolves typed
inputs before execution, never raw observations on the hot path.

Logical Evidence Store ownership includes observations, exposures and outcomes.
Current implementation uses `IInputTelemetrySink`, `IInputEvidenceReader` and
`IInputEvidenceSnapshotStore`, plus the state library's
`IConfirmedExposureReader`. #49 consolidates executable server boundaries.
Current outcome bindings retain attributed frames, not a general observation
warehouse or separately queryable Outcome log.

## OTLP/HTTP

```text
POST /otlp/{appId}/{environment}/v1/metrics
POST /otlp/{appId}/{environment}/v1/traces
POST /otlp/{appId}/{environment}/v1/logs
```

Only binary `application/x-protobuf`, uncompressed/identity or gzip, is
supported. JSON OTLP, gRPC and profiles are unsupported.
Wire types use pinned upstream `opentelemetry-proto` v1.11.0; provenance and
license live beside the host proto files.

`polari.telemetry:ingest` is separate from decide/confirm scopes.
Authenticated `polari_tenant_id`, `polari_app_id` and `polari_environment`
authorize route scope; resource attributes cannot broaden it. Trusted local
examples use the distinct `Flaggo-Local-Telemetry` authorization header,
never a public bypass.

| HTTP | Native result |
| --- | --- |
| 200 | Signal-specific export response after durable acceptance |
| 200 partial success | Rejected record count and bounded diagnostics; not a retry request |
| 400 | Malformed Protobuf/gzip/native structure |
| 401/403 | Authentication or authorized-scope denial |
| 413 | Encoded/decompressed bytes or decoded-work limit |
| 415 | Unsupported media type/encoding |
| 429/503 | Capacity or durable availability failure, `Retry-After: 1` |

Errors are native `google.rpc.Status`, not JSON Problem Details. Unknown
optional fields retain Protobuf behavior. Unmatched records are discarded with
counters. If multiple bindings match, count a native record rejected only when
none accept it; individual binding failures remain diagnostics. Response
diagnostics are bounded to 2,048 characters.

## Configuration and Collector

`Flaggo:Telemetry:*` settings use double underscores in environment variables.
All limits are positive integers.

| Setting | Default |
| --- | --- |
| `MaximumBodyBytes` | 4 MiB independently for encoded and decompressed bodies |
| `MaximumRecords` | 1,000 decoded records/work items, including span events |
| `MaximumBindings` | 1,000 active bindings |
| `MaximumFrames` | 10,000 retained target/stream frames |
| `MaximumSnapshotBytes` | 16 MiB |
| `CommitDescriptorPath` | `data/telemetry/inputs.commit.json`, relative to host content root |

The [Collector configuration](../../../examples/otel-evidence/collector.yaml)
uses stock `otelcol` 0.161.0 with `otlp_http/flaggo`, `proto`, gzip and base URL
ending `/otlp/<app>/<environment>`. The exporter appends `/v1/<signal>`.
Receivers bind loopback; independent existing-backend pipelines remain intact.
Credentials belong to Collector configuration, not manifests/catalogs/browsers.

## Projection and attribution

[Evidence](../../architecture/EVIDENCE.md) specifies latest Gauge, span
duration/attribute, named span-event and structured-log scalar projections.
Bindings declare meaning/type/unit/range, selectors, exact target, source-time
freshness, observed-only coverage and attribution. Required input bindings use
attribution `none`; confirmed bindings are outcome/objective evidence.

Only a completed exposure may satisfy attributed span/log evidence.
Lookup checks authenticated scope, exact definition/revision/digest and
resolved target. Pending receipts, an audit-only preparation, bare decision ID,
trace context or Gauge exemplar are insufficient. Failure never fabricates a
link or silently reclassifies the record as a successful attributed outcome.

Exact replay retains validated attribution even after the transient
confirmation store restarts. Previously unseen references to lost
confirmations fail closed. Source timestamps, not arrival/retry time, determine
age; no sampling flag becomes complete-population evidence or confidence.

## Durability and failure

Ingestion serializes bounded publication, pins approved non-retired bindings,
commits through the snapshot writer, then swaps the immutable read view before
acknowledging. Rejected capacity cannot evict fresh evidence. Host shutdown
drains disposal and releases the writer lease. See
[Evidence Store](../evidence-store/README.md) for restart and uncertain-write
semantics.

Missing fields, unsupported type/projection, unit mismatch, invalid target and
unusable freshness are explicit errors, never inferred zeroes. Required
unusable runtime operands produce 503 with SDK fallback forbidden. Separate
constraint-quality evidence retains its own declared behavior.

Native domain/HTTP tests cover scope, projection, freshness, replay, ambiguity,
invalidations, atomic generations, capacity, gzip, partial success and
confirmation. The [real Collector example](../../../examples/otel-evidence/README.md)
proves the complete cloud-free flow. No warehouse, query language, rolling
statistics, sampling correction or request-time AI is introduced.
