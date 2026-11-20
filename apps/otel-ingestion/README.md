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

The reusable `flaggo-otlp-http` router accepts an injected inbox and independent
security policy without depending on SQLite. The local binary explicitly uses
`TrustedLocal`. Each Resource must declare unique, valid `flaggo.tenant`,
`flaggo.application` and `flaggo.environment` string attributes. Every declared
scope must be authorized before the complete payload and
`flaggo.otlp.authorization/v1` envelope are appended atomically. One forbidden
or missing scope rejects the entire batch without writes. OTel Ingestion does not:

- derive a signal `SourceKey` or `MaterializationRoute`;
- split one request by Resource or signal item;
- decide whether telemetry is relevant to a current contract;
- materialize or correlate evidence; or
- delete data because another app has processed it.

Evidence Materializer rechecks the persisted authorization envelope before
decomposition, route admission and Evidence Store projection. Private identity
provider integration remains outside the local composition. Public readiness
omits inbox inventory; the local binary explicitly opts into detailed health.
Inbox schema 1 requires the [explicit migration](../../docs/design/architecture/CORE_EXTRACTION.md#implemented-breaking-wire-cutover);
legacy bytes remain unverified rather than being relabelled authenticated.

Non-local composition can use
`flaggo_otlp_http::router_with_private_health(inbox, receiver, security)` without
supplying a retention policy. The [shared receiver](../../modules/otlp-http/README.md)
also calls the default-accepting batch admission hook after full authorization
and before append. Metered adapters charge atomically once per distinct tenant;
empty exports charge no tenant and retain existing append behavior. Local
`TrustedLocal` remains unmetered with the same detailed health and retention.

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
