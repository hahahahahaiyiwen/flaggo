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

Phase 3 local integration uses a strict JSON-file evidence adapter selected by
`Flaggo__Evidence__LocalFilePath`. It maps an activated strategy ID to a
deterministic evidence snapshot and reloads the file for each decision.
Missing strategy evidence remains `null`. Missing, unreadable, malformed, or
invalid files cross the module boundary as `EvidenceUnavailableException` and
make evidence health unavailable. Reasoning handles that explicit failure
exactly like a missing snapshot under the registered definition's effective
required-evidence and client-fallback policy; unrelated I/O failures are not
reclassified as evidence failures. Persisted format version and
`evidenceQuality` are required scalars; their presence is validated separately
so omitted values are rejected without rejecting legitimate zero values.

Update this document when signal ownership, aggregation, or quality semantics
change.
