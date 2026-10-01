# Raw OTLP Inbox

This Rust crate owns the bounded durable work log between the synchronous OTLP
Receiver and the asynchronous Evidence Materializer.

## Boundary

`RawOtlpInbox` is storage-neutral. It accepts one already-validated,
decompressed OTLP export request and exposes:

- durable append receipts;
- monotonic batch-cursor reads for replay and materialization; and
- capacity, retention, replay-boundary, and expiration health.

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
- Appends serialize before retention and capacity evaluation.
- Retained batch and payload-byte counters update in the same transaction, so
  capacity admission does not scan the full inbox.
- Expired rows and their replay diagnostics commit even when the incoming
  request is rejected for capacity.
- Capacity pressure never evicts a non-expired batch.
- Reads verify stored payload length, metadata, and SHA-256.
- Unsupported schema versions fail explicitly; the adapter does not migrate or
  reinterpret them.

Defaults are 1 GiB of retained decompressed payload bytes and 24 hours of hard
retention. Both are provided through `RawOtlpInboxLimits`.

Receiver parsing, decompression, size enforcement, and OTLP HTTP responses
belong to `apps/otel-ingestion`. Evidence interpretation and compaction belong
to the future Evidence Materializer.
