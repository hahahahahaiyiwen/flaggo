# Applications

`apps` contains independently runnable composition roots. The active service
composition roots are:

```text
apps/contract-service
apps/decision-service
apps/evidence-materializer
apps/otel-ingestion
```

Contract Service owns management routes. Decision Service owns decide and
health routes. OTel Ingestion owns the `/v1/logs`, `/v1/metrics`, and
`/v1/traces` telemetry endpoints. It durably appends each complete valid export
request to the bounded Raw OTLP Inbox before acknowledgement. Evidence
Materializer is a standalone Rust worker that replays the inbox through
versioned checkpoints and transactionally writes selected query-ready
observations, provenance, conflicts, and diagnostics to Evidence Store.
Contract-specific association is deferred to analysis. Reusable strict HTTP
mechanics live under `modules/hosting`; apps do not reference another app
project.

Evidence Materializer and OTel Ingestion share `FLAGGO_DATABASE_URL` (default
`sqlite://flaggo.db`). Set `FLAGGO_CONTRACT_CATALOG_URL` to the Contract Service
`/v3/decision-contract-catalog/current` endpoint to enable ordinary application
evidence routes across all authorities; an optional
`FLAGGO_CONTRACT_CATALOG_BEARER_TOKEN` supplies non-development
authentication. Without that endpoint, the worker keeps materializing strictly
valid built-in Flaggo protocol observations using its last durable catalog or
an empty route catalog.

The Phase 4 Evidence Materializer strictly derives tenant, application, and
environment from `flaggo.*` OTLP Resource attributes. The receiver does not
require authentication. Authentication and authorization for telemetry writes
are deferred to a later phase.

The pre-v3 shared host stack was removed after the v3 consumer cutover.
Neither service exposes forwarding projects, compatibility routes, or
alternate legacy APIs.

Service readiness requires each store's exact schema version and readable
owned tables and columns. It does not scan every stored artifact. Exact
contract and executable integrity remains enforced when an authority record is
read, and Decision Service materializes the selected executable before
evaluation.

Decision Service readiness does not require an Evidence module, OTLP routes, a
Collector, async analysis, or durable decision append.

Both hosts configure the same local SQLite database path/connection string.
Each store module owns its tables and exact schema version; hosts do not issue
SQL directly.
