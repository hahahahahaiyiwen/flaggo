# Evidence Materializer

**Role:** implemented asynchronous background worker.

Evidence Materializer is the composition root that turns retained OTLP export
requests into query-ready observations. It reads the Raw OTLP Inbox forward
through one versioned checkpoint, decodes and routes individual signals, and
transactionally writes Evidence Store projections.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Read pending Raw OTLP Inbox batches in receipt order.
- Decompose OTLP Resources and signal items.
- Derive declared tenant, application, and environment from required
  `flaggo.*` Resource attributes.
- Recognize built-in Flaggo protocol observations.
- Fetch and durably cache the current contract catalog when configured.
- Compile ordinary application-telemetry routes from that catalog.
- Derive logical observation identity and canonical content digests.
- Deduplicate exact repeats and record conflicting content explicitly.
- Atomically commit each processed batch page's observations, inbox provenance,
  diagnostics, conflicts, and forward checkpoint.
- Persist each validated catalog update independently from batch projection.
- Emit structured progress, backlog, freshness, and fatal-failure events.

## Boundaries

Evidence Materializer does not:

- accept or acknowledge OTLP requests;
- delete or retain Raw OTLP Inbox batches;
- revisit completed batches when the contract catalog changes;
- eagerly associate reusable observations with contracts;
- correlate decisions and outcomes or decide whether telemetry is usable
  learning evidence;
- generate executable candidates; or
- validate or activate executables.

Catalog unavailability retains the last valid durable catalog. With no catalog,
the worker still recognizes supported built-in Flaggo observations and uses an
empty route map for ordinary application telemetry.

Catalog activation and batch projection are separate durable writes. A catalog
may be cached while the forward checkpoint remains unchanged; the next batch
transaction applies the currently active catalog and advances the checkpoint
only with its complete projection.

## State and dependencies

| Boundary | Access | Purpose |
| --- | --- | --- |
| Raw OTLP Inbox | Read only | Consume retained pending export requests after the checkpoint |
| Evidence Store | Owner and writer | Observations, materialization provenance, diagnostics, conflicts, checkpoint, and catalog cache |
| Contract Service catalog API | Optional read | Refresh current application-telemetry routes with ETag validation |

The Raw OTLP Inbox and Evidence Store currently share one SQLite database URL,
but they retain separate schemas and owners. Exactly one Evidence Materializer
may be active per database.

## Execution model

The architecture diagram represents this app as a diamond because it is
poll-driven background work rather than a request-serving API. The node is a
conceptual worker boundary, not a promise that all future deployments use one
process. The current persistence contract nevertheless requires one active
materializer for each database.

The process exposes operational state through structured events rather than an
HTTP health endpoint. Startup and committed-page events include checkpoint,
pending-batch count, oldest pending age, newest evidence time, and evidence
freshness.

## Configuration

| Environment variable | Meaning |
| --- | --- |
| `FLAGGO_DATABASE_URL` | Shared SQLx SQLite database URL; defaults to `sqlite://flaggo.db` |
| `FLAGGO_CONTRACT_CATALOG_URL` | Optional Contract Service current-catalog endpoint |
| `FLAGGO_CONTRACT_CATALOG_BEARER_TOKEN` | Optional bearer token; requires a catalog URL |
| `FLAGGO_MATERIALIZER_POLL_INTERVAL_MS` | Inbox polling interval; defaults to 250 ms |
| `FLAGGO_MATERIALIZER_CATALOG_INTERVAL_SECONDS` | Catalog refresh interval; defaults to 30 seconds |

## Run

From the repository root:

```powershell
cargo run --locked --bin flaggo-evidence-materializer
```

## Verify

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Related documents

- [Evidence and learning](../../docs/design/architecture/EVIDENCE.md)
- [Domain and store modules](../../modules/README.md)
- [Contract Service](../contract-service/README.md)
- [OTel Ingestion](../otel-ingestion/README.md)
