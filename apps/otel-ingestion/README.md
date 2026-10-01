# OTel Ingestion

This directory is the Rust executable boundary for OTel Ingestion.

The host composes an OTLP/HTTP Receiver with the storage-neutral Raw OTLP Inbox
contract and its initial SQLx/SQLite adapter. It initializes the inbox-owned
schema before listening, accepts logs, metrics, and traces at `/v1/logs`,
`/v1/metrics`, and `/v1/traces`, and reports inbox capacity and replay health
through `/health/ready`.

The receiver supports Protobuf JSON and binary protobuf with identity or gzip
transport encoding. It validates the signal-specific export request, enforces
the configured limit after decompression, and acknowledges success only after
the complete decompressed payload is durably appended to the inbox.

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
| `FLAGGO_OTLP_MAXIMUM_DECOMPRESSED_REQUEST_BYTES` | Per-request decompressed-byte limit; defaults to 64 MiB |
| `FLAGGO_OTLP_INBOX_MAX_PAYLOAD_BYTES` | Maximum retained payload-byte total |
| `FLAGGO_OTLP_INBOX_HARD_RETENTION_SECONDS` | Hard raw-batch retention |

## Verify

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
