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
Consume with `default-features = false` to exclude the SQLite adapter and its
SQLx dependency entirely. The default `sqlite` feature retains local behavior;
the OTLP codec depends only on the contract-only form.

`SqliteRawOtlpInbox` is the initial adapter. It owns schema component
`raw-otlp-inbox` version `1` in the shared `flaggo_schema_versions` table and
stores payloads in `raw_otlp_inbox_batches`.

## Invariants

### Capacity failures

`RawOtlpInboxError::CapacityExceeded` retains its existing payload-byte fields
and behavior. Do not put row counts or metadata bytes in those fields.
Adapters can instead return:

```rust
RawOtlpInboxError::ResourceCapacityExceeded {
    resource: InboxCapacityResource::Rows, // or MetadataBytes
    retained,
    incoming,
    maximum,
}
```

`InboxCapacityResource` is a root export; all three counters are `u64` in the
selected unit. This is known exhaustion, distinct from dependency
`Unavailable`. Capacity rejection must atomically leave no persisted batch or
partial inbox-capacity charge. It does not refund an earlier request-admission
charge. The shared OTLP receiver returns sanitized HTTP 429/code 8 without
exposing the resource or counters and without inventing `Retry-After`.
Existing payload-byte 429 and dependency 503 responses retain their headers.

The seam adds no production limits or retention policy and does not implement
row/metadata accounting in SQLite. Hosted adapters must supply that accounting,
including for empty exports; payload-byte limits alone cannot bound empty rows.

### Storage behavior

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
These are trusted-local defaults, not approved hosted capacity or retention
policy. This extraction adds no authenticated-envelope or distributed worker
fencing guarantee.

Receiver parsing, decompression, size enforcement, and OTLP HTTP responses
belong to `apps/otel-ingestion`. Resource decomposition, routing, evidence
interpretation, and projection belong to the Evidence Materializer.
