# Raw OTLP Inbox

This Rust crate owns the bounded durable work log between the synchronous OTLP
Receiver and the asynchronous Evidence Materializer.

## Boundary

`RawOtlpInbox` is storage-neutral. It accepts one already-validated,
decompressed OTLP export request and exposes:

- durable append receipts;
- monotonic batch-cursor reads for replay and materialization; and
- capacity, retention, replay-boundary, and expiration health.

`RawOtlpInboxRetention` exposes retention as a separate explicit maintenance
operation. OTel Ingestion owns its schedule; materializer reads do not invoke
it.

The contract uses OTLP and inbox domain types only. It does not expose SQLx,
SQLite connections, table names, or transactions.

`SqliteRawOtlpInbox` is the initial adapter. It owns schema component
`raw-otlp-inbox` version `1` in the shared `flaggo_schema_versions` table and
stores payloads in `raw_otlp_inbox_batches`.

## Invariants

- The stored payload is the exact decompressed request body.
- Receipt time and the monotonic batch ID are assigned inside the serialized
  append transaction; receipt time never moves backward.
- Media type is derived from the validated wire encoding.
- Payload length and SHA-256 are calculated by the inbox.
- An append commits before its receipt is returned.
- Appends never expire or delete retained rows.
- Replay reads and health inspection are side-effect free and require no
  retention configuration.
- Explicit retention maintenance serializes with appends and transactionally
  updates retained counts, expiration counts, and the earliest replay boundary.
- Retained batch and payload-byte counters update in the same transaction, so
  capacity admission does not scan the full inbox.
- Capacity pressure never evicts a batch; appends remain retryable until
  explicit retention maintenance releases capacity.
- Reads verify stored payload length, metadata, and SHA-256.
- Unsupported schema versions fail explicitly; the adapter does not migrate or
  reinterpret them.
- Inbox rows do not contain application-declared authority, source keys,
  materialization routes, or contract associations. One row may contain
  several OTel Resources and authority scopes.

Defaults are 1 GiB of retained decompressed payload bytes and 24 hours of hard
retention. Both are provided through `RawOtlpInboxLimits`.

Receiver parsing, decompression, size enforcement, and OTLP HTTP responses
belong to `apps/otel-ingestion`. Resource decomposition, routing, evidence
interpretation, and projection belong to the Evidence Materializer.
