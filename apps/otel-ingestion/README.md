# OTel Ingestion

This directory is the Rust executable boundary for OTel Ingestion.

The host composes the storage-neutral Raw OTLP Inbox contract with its initial
SQLx/SQLite adapter. It initializes the inbox-owned schema before listening and
reports inbox capacity and replay health through `/health/ready`.

The next issue #53 slice adds the OTLP Receiver routes that validate and append
complete logs, metrics, and traces requests through this contract.

The existing .NET ingestion project remains only until the Rust receiver
reaches contract parity. It will then be removed rather than retained as a
fallback or parallel implementation.

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
| `FLAGGO_OTLP_INBOX_MAX_PAYLOAD_BYTES` | Maximum retained payload-byte total |
| `FLAGGO_OTLP_INBOX_HARD_RETENTION_SECONDS` | Hard raw-batch retention |

## Verify

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
