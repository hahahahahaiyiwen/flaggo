# Evidence Module

This internal library implements Evidence Store query/projection capabilities:
native-observation projection, bounded materialized input snapshots,
source-time freshness and provenance, and a separate policy-quality boundary.
Application instrumentation and Collector configuration remain outside Flaggo.
OTel Ingestion owns server intake; Decision Service appends
decision/exposure records through current audit-named adapters.

It returns domain evidence types through async ports and does not select
actions. Missing evidence remains distinguishable from low-quality evidence,
and detailed evidence stays in Evidence Store rather than compact runtime
responses.

## Current implementation

`src/Flaggo.Evidence` owns `IEvidenceProvider`, `IEvidenceHealth`,
`DecisionEvidenceRequest`, and the deterministic `InMemoryEvidenceProvider`.
Reasoning receives the provider through constructor injection and has no
concrete evidence dependency.

`IInputTelemetrySink` receives host-normalized observations under authenticated
tenant/application/environment. `InputEvidenceMaterializer` consumes approved
registry bindings and committed-exposure lookup, reduces Gauge/span/span-event/
log data to declared latest scalar values, and durably publishes one bounded
immutable generation through `IInputEvidenceSnapshotStore`.
`IInputEvidenceReader` pins one generation and evaluation time for the complete
declared operand batch. It never queries raw telemetry on the decision path.

Source nanoseconds, not export/arrival time, determine inclusive freshness.
Retries and older data cannot refresh age. Exact retries of retained validated
frames preserve their attribution after restart; previously unseen references
still require a live confirmation. Conflicting latest values and multiple fresh
Gauge streams are ambiguous; invalid newer observations
invalidate last-good values. Scope, exact definition, binding, target, and
stream partition frames. Observed coverage never becomes learned confidence.
The host supplies an opaque metric stream fingerprint that preserves native
attribute types and is independent of attribute ordering. Scalar projection
does not reinterpret binary/structured attributes as numeric or string values.

`LocalInputEvidenceStore` uses verified committed-file publication with one
writer lease, frame/byte capacity, strict restart validation, and fail-closed
recovery after uncertain writes. Required unusable inputs are errors, not
implicit values or SDK fallback. Read-only lookup does not evict fresh frames.
The host owns binary OTLP parsing/authentication and transport limits; see
[OTel Ingestion](../../docs/design/otel-ingestion/README.md) and
[Evidence Store](../../docs/design/evidence-store/README.md).

The separate policy-quality path uses a strict JSON-file adapter selected by
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

Request-only decisions without evidence policy use neither evidence port.
Update this document when input ownership, projection, persistence, or quality semantics
change.
