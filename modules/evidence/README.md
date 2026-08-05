# Evidence Module

Owns signal ingestion boundaries, evidence snapshots, aggregation lineage, and
quality information consumed by runtime and asynchronous intelligence.

It returns domain evidence types through async ports and does not select
actions. Missing evidence remains distinguishable from low-quality evidence,
and detailed evidence stays in audit rather than compact runtime responses.

## Current implementation

`src/Flaggo.Evidence` owns `IEvidenceProvider`, `IEvidenceHealth`,
`DecisionEvidenceRequest`, and the deterministic `InMemoryEvidenceProvider`.
Reasoning receives the provider through constructor injection and has no
concrete evidence dependency.

Update this document when signal ownership, aggregation, or quality semantics
change.
