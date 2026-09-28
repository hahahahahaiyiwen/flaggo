# Applications

`apps` contains independently runnable composition roots. Phase 3 has:

```text
apps/contract-service
apps/decision-service
```

Contract Service owns management routes. Decision Service owns decide and
health routes. Reusable strict HTTP mechanics live under `modules/hosting`;
apps do not reference another app project.

The pre-v3 shared host stack was removed after the v3 consumer cutover.
Neither service exposes forwarding projects, compatibility routes, or
alternate legacy APIs.

Service readiness requires each store's exact schema version and readable
owned tables and columns. It does not scan every stored artifact. Exact
contract and executable integrity remains enforced when an authority record is
read, and Decision Service materializes the selected executable before
evaluation.

Readiness does not require an Evidence module, OTLP routes, a Collector, async
analysis, or durable decision append.

Both hosts configure the same local SQLite database path/connection string.
Each store module owns its tables and exact schema version; hosts do not issue
SQL directly.
