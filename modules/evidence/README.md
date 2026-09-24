# Evidence Module

This current internal library implements Evidence Store query/projection
capabilities. OTel Ingestion owns server intake; Decision Service appends
decision/exposure records through current audit-named adapters.

The module owns evidence snapshots, aggregation lineage, and quality
information consumed by runtime and Async Analysis Pipeline.

It returns domain evidence types through async ports and does not select
actions. Missing evidence remains distinguishable from low-quality evidence,
and detailed evidence stays in Evidence Store rather than compact runtime
responses.

## Current implementation

`src/Flaggo.Evidence` owns `IEvidenceProvider`, `IEvidenceHealth`,
`DecisionEvidenceRequest`, and the deterministic `InMemoryEvidenceProvider`.
Reasoning receives the provider through constructor injection and has no
concrete evidence dependency.

Phase 3 local integration uses a strict JSON-file evidence adapter selected by
`Flaggo__Evidence__LocalFilePath`. The configured path is a commit descriptor,
not raw evidence JSON. It maps an activated strategy ID to a deterministic
evidence snapshot and reloads the committed artifact for each decision.
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
Each reload uses the shared committed-file protocol through the module-owned
`IEvidenceSnapshotProvider` seam. A directly constructed adapter resolves its
configured source for each lookup. The ASP.NET host injects a request-scoped
provider: a direct descriptor is pinned for that request, while bootstrap mode
returns the evidence reference from the same once-resolved generation used by
state. Stable length/digest/path/version corruption fails closed, and a
parseable in-place mutation cannot cross the evidence boundary. Independent
direct state/evidence descriptors are not a cross-file transaction; coordinated
publication uses the generation manifest.

Trusted direct writers use `CommittedFileSnapshotWriter`, which fsyncs a new
immutable artifact, syncs its parent directory, fsyncs the descriptor
temporary, renames the descriptor last, and syncs the directory again.
Generation writers publish all immutable siblings before atomically switching
`current.json`. Direct unpinned files are never accepted.
The default artifact bound is 16 MiB and the normal stable path has no timer
delay.

Update this document when signal ownership, aggregation, or quality semantics
change.
