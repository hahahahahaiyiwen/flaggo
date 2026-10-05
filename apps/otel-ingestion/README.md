# OTel Ingestion

**Role:** implemented synchronous OTLP/HTTP API with an inbox-retention
background worker.

OTel Ingestion is the composition root for accepting telemetry into Flaggo. It
validates one complete transport request and durably appends its decompressed
payload to the Raw OTLP Inbox before acknowledging success.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Accept OTLP logs, metrics, and traces over HTTP.
- Support Protobuf JSON and binary protobuf with identity or gzip transport
  encoding.
- Validate the signal-specific export-request envelope.
- Enforce the configured decompressed request-size limit.
- Append the complete accepted payload and transport metadata atomically.
- Report liveness plus inbox capacity and replay readiness.
- Invoke the inbox-owned receipt-age retention operation at startup and
  periodically afterward.

The receiver request path only appends. Retention is a separate background
role hosted by the same binary today. It runs at least once per minute, or once
per retention period when that period is shorter, and does not consider
materialization progress.

## Boundaries

Accepted export requests are opaque durable work items after transport and
message-shape validation. One request may contain several OTel Resources and
declared authority scopes. OTel Ingestion does not:

- extract or validate application-declared authority;
- derive a signal `SourceKey` or `MaterializationRoute`;
- split one request by Resource or signal item;
- decide whether telemetry is relevant to a current contract;
- materialize or correlate evidence; or
- delete data because another app has processed it.

Evidence Materializer owns decomposition, declared authority extraction, route
admission, and Evidence Store projection. Authentication and authorization for
telemetry writes remain deferred.

## Interfaces

```text
POST /v1/logs
POST /v1/metrics
POST /v1/traces
GET  /health/live
GET  /health/ready
```

## State and dependencies

OTel Ingestion is the semantic owner of inbox append, capacity, and retention.
It composes the Raw OTLP Inbox module, which owns the persistence contract,
schema, and SQLite adapter. Evidence Materializer receives read-only access
through that contract. OTel Ingestion has no request-path dependency on
Contract Service, Decision Service, Evidence Store, or Async Analysis.

The architecture diagram represents the receiver as a rectangle and retention
as a diamond because they have different execution models. They are conceptual
roles rather than a required deployment split; the current Rust process hosts
both.

## Run

```powershell
cargo run --locked --bin flaggo-otel-ingestion
```

The default listening address is `127.0.0.1:5090`. Override it with
`FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS`; use `127.0.0.1:0` to request an
ephemeral loopback port.

The default database URL is `sqlite://flaggo.db`. Inbox bounds default to 1 GiB
of retained decompressed payload bytes and 24 hours of hard retention.

| Environment variable | Meaning |
| --- | --- |
| `FLAGGO_DATABASE_URL` | SQLx SQLite database URL |
| `FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS` | Receiver socket address; defaults to `127.0.0.1:5090` |
| `FLAGGO_OTLP_MAXIMUM_DECOMPRESSED_REQUEST_BYTES` | Per-request decompressed-byte limit; defaults to 64 MiB |
| `FLAGGO_OTLP_INBOX_MAX_PAYLOAD_BYTES` | Maximum retained payload-byte total |
| `FLAGGO_OTLP_INBOX_HARD_RETENTION_SECONDS` | Hard raw-batch retention |

## Operational observability

The service uses instrumentation scope `flaggo.otel-ingestion`. OTLP append
and inbox-retention boundaries emit profile-aligned spans, structured events,
request and payload-size metrics, retention counters, and current inbox
gauges. Incoming W3C `traceparent` is attached to the append span when valid.
Rejected requests log bounded transport facts without recording payloads.

Structured console logging is always enabled. Logs, metrics, and traces export
as OTLP/HTTP binary Protobuf only when standard
`OTEL_EXPORTER_OTLP_*_ENDPOINT` configuration is present. Configuration that
would export back into the process's own local receiver is rejected to prevent
recursive ingestion.

## Verify

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Related documents

- [Evidence and learning](../../docs/design/architecture/EVIDENCE.md)
- [Raw OTLP Inbox module](../../modules/raw-otlp-inbox/README.md)
- [Evidence Materializer](../evidence-materializer/README.md)
- [Internal service observability](../../docs/design/architecture/OBSERVABILITY.md)
