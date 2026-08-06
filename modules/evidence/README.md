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
The complete file is structurally validated before typed deserialization:
malformed or trailing data and case-sensitive duplicate members in the root,
strategy map, snapshots, or nested details are rejected, while separate
sibling snapshots may use the same member names.
Each reload holds a read snapshot that shares reads and deletion but denies
in-place writes, preventing cooperating writers from truncating or rewriting
bytes while they are copied on Windows and Linux. Writers must create and
fully flush a sibling temporary file, close it, then atomically replace the
configured path; replacement remains allowed while an older snapshot is open,
and the next decision observes the replacement. .NET writers can use
`File.Replace` on the same volume after closing the replacement stream.
Native Unix writers must
follow this atomic-replacement protocol because file sharing is advisory
outside cooperating runtimes.

Update this document when signal ownership, aggregation, or quality semantics
change.
