# Evidence Store design

## Purpose

Evidence Store is the durable record boundary for what Flaggo observed, what it
returned, what the application confirmed it used, and what outcomes followed.

It replaces separate conceptual ownership for telemetry evidence and audit.
Reconstructability is an invariant of stored records, not a standalone Audit
service.

## Record kinds

Evidence Store distinguishes immutable record kinds:

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
  resolved typed values, never an EvidenceSnapshot. Future evidence-dependent
  constraints require a separately accepted binding/view contract before they
  may read additional evidence.
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

Every record carries application/environment scope, exact definition identity
when applicable, target identity, timestamp, and provenance. Decision records
remain immutable so historical behavior is explainable after definitions or
authority change.

## Current implementation mapping

`Flaggo.Evidence` and `Flaggo.Audit` currently provide separate ports and local
adapters. Issue #40 owns their executable composition under this durable store
boundary; #47 changes logical ownership and documentation only.

## Related documents

- [Decision evidence](../../architecture/EVIDENCE.md)
- [Decision Service](../decision-service/README.md)
- [OTel Ingestion](../otel-ingestion/README.md)
- [Async Analysis Pipeline](../async-analysis/README.md)
