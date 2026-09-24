# Evidence Store design

## Purpose

Evidence Store is the durable record boundary for what Flaggo observed, what it
returned, what the application confirmed it used, and what outcomes followed.

It replaces separate conceptual ownership for telemetry evidence and audit.
Reconstructability is an invariant of stored records, not a standalone Audit
service.

## Record kinds

The logical Evidence Store distinguishes these record kinds; current adapters
implement only the bounded subset described below:

| Record | Meaning |
| --- | --- |
| Observation | Normalized OTel metric, span, or structured log/event with provenance. |
| Evidence view | Derived target/window/filter projection with freshness and quality. |
| Decision record | Exact inputs, authority lineage, constraints, fallback, and returned value. |
| Exposure record | Idempotent confirmation that the application applied or rendered a decision. |
| Outcome | Observation attributed to a confirmed exposure. |

These records answer different questions and must not be collapsed into one
generic event.

## Writers

- OTel Ingestion appends observations.
- OTel Ingestion validates declared outcome bindings and confirmed exposure
  identity, then appends exposure-linked outcomes.
- Decision Service appends decision records before server success.
- Decision Service appends exposure records after explicit confirmation.
- Async Analysis Pipeline may persist derived views through an owned projection
  port.

Successful writes in a ready configuration cross a process-failure-surviving
durability boundary. Volatile memory or console output is valid only for tests
or explicitly non-ready debugging.

## Readers

- Decision Service input resolution may read authorized materialized views for
  accepted evidence-sourced input declarations. The bounded executor receives
  resolved typed values, never an EvidenceSnapshot. Separately declared
  constraint-quality evidence uses its own explicit port and cannot be inferred
  from observed input coverage.
- Async Analysis Pipeline reads observations, views, decisions, exposures, and
  outcomes for future candidate production.
- Query tools and future operator clients read records through bounded query
  APIs.

## Explanation

Explanation is generated deterministically from structured decision facts:
selected authority, input values, applied constraints, fallback reason, and
recorded evidence when present. Free-form model reasoning is not required to
reconstruct a decision.

## Retention and identity

Every record carries authorized tenant/application/environment scope, exact definition identity
when applicable, target identity, timestamp, and provenance. Decision records
remain immutable so historical behavior is explainable after definitions or
authority change.

## Current implementation mapping

`Flaggo.Evidence` and `Flaggo.Audit` currently provide separate ports and local
adapters. #49 owns executable consolidation under this boundary; #47 changed
logical ownership and documentation, not #44's manifest/input contract.

### Materialized input snapshots

`IInputTelemetrySink` receives host-normalized observations;
`IInputEvidenceSnapshotStore` persists bounded frames;
`IInputEvidenceReader` supplies one immutable generation/evaluation time for
the full runtime input batch. Current storage is a latest-scalar snapshot, not
a raw telemetry warehouse or a separately queryable Outcome log.

Frame identity includes authenticated tenant/app/environment, exact
definition/revision/digest, binding and target; Gauge frames also carry typed
native stream identity. No implicit revision reuse occurs.
Only approved non-retired bindings participate.

`LocalInputEvidenceStore` holds one writer lease. Ingestion builds a bounded
candidate, publishes an immutable artifact through `CommittedFileSnapshotWriter`
and switches the read view only after commit. Expired frames may be reclaimed
on ingestion; fresh frames cannot be silently evicted to satisfy capacity.

Restart verifies descriptor/path/length/digest/version and the entire snapshot.
Uncertain publication invalidates reads until verified reload. Repair/restore
the committed artifacts and restart; never erase a corrupt store to fabricate
an empty success. Competing writers and oversized snapshots fail explicitly.
Freshness uses source nanoseconds, not export/file/retry time.
Exact replay cannot refresh age; ambiguous or newer invalid observations do
not silently retain a last-good operand.

### Decision and exposure recording

The current `LocalFileAuditSink` is the durable Evidence Store append adapter
for decision/exposure records, not an Audit service. Decision Service
preallocates record identity and returns success only after durable append;
the sink does not substitute identity. SDK fallback has no server record.
Confirmation capabilities are removed before any record sink.

Records preserve caller/resolved vectors and evidence source/time/target
provenance, including targets outside the selected state path. The local sink
rejects evidence provenance without resolved target information. Full
authority/activation-lineage integration remains downstream.

The sink uses bounded segments, a digest-pinned durable manifest, exclusive
writer lease, strict record parsing and replay markers. Torn/unlisted/corrupt
files or uncertain writes fail closed. Its invariants and inspection details
live in the [audit implementation README](../../../modules/audit/README.md),
not in a parallel component design.

Exposure preparation, append and final confirmation are separate transitions.
Only a completed confirmation can authorize attributed telemetry. Current
confirmation lookup lives in the state library and is in-memory; #49 moves
executable ownership. Previously validated durable frames retain their
provenance after restart, but new references to lost confirmations fail.
Decision-time inputs cannot be replaced during confirmation.

## Related documents

- [Decision evidence](../../architecture/EVIDENCE.md)
- [Decision Service](../decision-service/README.md)
- [OTel Ingestion](../otel-ingestion/README.md)
- [Async Analysis Pipeline](../async-analysis/README.md)
