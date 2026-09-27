# Applications

`apps` contains independently runnable composition roots. Phase 3 has:

```text
apps/contract-service
apps/decision-service
```

Contract Service owns management routes. Decision Service owns decide and
health routes. Reusable strict HTTP mechanics live under `modules/hosting`;
apps do not reference another app project.

The legacy `data-plane`, `control-plane`, and shared host layer were removed
after the v3 consumer cutover. Neither service exposes forwarding projects,
compatibility routes, or alternate v1 APIs.

Decision Service readiness requires exact Contract Store reads, Executable
Store active lookup, and the bounded evaluator. It does not require an
Evidence module, OTLP routes, a Collector, async analysis, or durable decision
append.

Both hosts configure the same local SQLite database path/connection string.
Each store module owns its tables and exact schema version; hosts do not issue
SQL directly.
