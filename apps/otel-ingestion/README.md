# OTel Ingestion

This directory is the Rust executable boundary for OTel Ingestion.

The bootstrap host establishes the Tokio/Axum process, language-neutral
listening notification, liveness endpoint, Cargo workspace, and test boundary.
The next issue #53 slice adds the OTLP Receiver and the SQLite-backed Raw OTLP
Inbox.

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

## Verify

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
