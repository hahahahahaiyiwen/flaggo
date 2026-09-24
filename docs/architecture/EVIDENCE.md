# Decision evidence

## Boundary

Evidence Store is the logical durable boundary for observations, materialized
views, decision records, confirmed exposures and outcomes. Evidence informs
Decision Service constraints and future Async Analysis Pipeline candidates;
it never creates authority.

Applications own OTel instrumentation, providers, exporters, Collectors,
retention and sampling. OTel Ingestion interprets selected existing records
through definition-local bindings. It does not introduce a parallel producer
schema or a telemetry warehouse.

| Kind | Meaning and current role |
| --- | --- |
| Context | Caller facts and identifiers verified by target resolution |
| Request input | Current primitive operand, with no instrument required |
| Observation | Received native metric, span/span-event or structured log |
| Input evidence view | Latest scalar projected for an exact binding/target |
| Constraint-quality evidence | Separate optional quality/model contract |
| Decision record | Reconstructable result persisted before server success |
| Exposure record | Explicit confirmation of application use |
| Outcome | Observation attributed to a completed confirmed exposure |

```text
application OTel pipeline -> authenticated OTel Ingestion
  -> registered binding projection -> durable immutable input generation
  -> Decision Service input resolution -> bounded rule + constraints
  -> durable decision record
```

## Supported projections

Every binding declares exact instrumentation scope (optional version),
resource/record selectors, type and meaning, target, `latest` projection,
source-time freshness, `sampling.accept: observed` and attribution.

| Native source | Supported value | Source time |
| --- | --- | --- |
| Metric Gauge | Scalar point with exact unit | Point timestamp |
| Span | Duration in milliseconds or primitive attribute | Span end |
| Named span event | Primitive event attribute | Event timestamp |
| Log | Primitive attribute or scalar at a structured body path | Log timestamp |

Logs select an event name or exact scalar body. Body paths traverse native
structured values, never JSON-looking text. Attribute namespaces remain
distinct. Missing fields, wrong types/units, no-recorded-value flags and
out-of-range values are errors, not zeroes.

Sums/counters, histograms, quantiles, rates, rolling windows, arbitrary queries,
text extraction and exemplar-based attribution are unsupported. A span event
or named log needs no Flaggo event declaration. Possible future evidence views
do not advertise these operations as current capabilities.

## Freshness and ambiguity

Source timestamps retain unsigned nanoseconds as decimal strings. Freshness
is inclusive: `0 <= evaluationTime - sourceTime <= maxAgeSeconds`. Missing
source time is not replaced by arrival time. Future observations and clock
regression fail freshness. The declared maximum age is a positive integer no
larger than `922337203685` seconds.

Exact redelivery is idempotent and older data cannot replace newer values or
refresh age. Conflicting latest values are ambiguous. Multiple fresh Gauge
streams for one binding target are ambiguous, without averaging or
last-writer-wins selection. Narrow selectors or target dimensions to one
series. Identifiable invalid newer observations invalidate last-good values.

Metric stream identity preserves native attribute types and disregards
attribute-map order. Span/log latest means the latest matching received
record, not necessarily the latest event that occurred.

## Scope, persistence and failure

Authenticated claims supply tenant/application/environment. Resource
attributes are metadata, not authorization. Current frames partition scope,
exact definition identity, binding and target; new revisions do not inherit
older frames implicitly.

Ingestion pins approved non-retired bindings and commits a bounded immutable
generation before acknowledging acceptance. Runtime input resolution pins one
generation and evaluation time for the full batch, with no raw scans, export
waits or per-operand refresh.

The local input store holds one writer lease and uses verified committed-file
publication. Restart validates the generation. Corruption or uncertain
publication invalidates reads until verified reload. Capacity failure cannot
evict fresh required inputs or acknowledge uncommitted work.

Required missing, stale, future, ambiguous, invalid or unavailable evidence
returns `503 required-evidence-unavailable` with binding-specific diagnostics
and `clientFallback.eligible: false`. Request-only decisions without
evidence-dependent constraints read neither evidence port. Deterministic
numeric execution returns `confidence: null`.

## Collection and sampling

Trace head sampling happens in application SDKs at span creation. Collector
tail sampling operates on received spans and may require trace affinity.
Batching, filtering, buffer limits and delivery loss can reduce observations.
Collector fan-out cannot recover records dropped upstream.

Metrics aggregate measurements into exported points independently of trace
sampling. Logs have independent filtering/delivery behavior. Keeping a log or
sampled span does not prove population completeness. Applications choose
Collector routing and retain their existing backend exports.

This slice accepts observed-only coverage. Flags and IDs are provenance, not
proof of unbiased sampling, complete counts, statistical confidence or model
quality.

## Decision, exposure and outcome

```text
Decision Service returns a durably recorded value
  -> application applies it -> explicit confirmation -> committed exposure
  -> attach confirmed attributes to an existing span/log
  -> OTel Ingestion validates the outcome binding and confirmation
```

Confirmation is operational and idempotent, never sampled telemetry.
Attribution checks authenticated scope, completed confirmation, exact
definition/revision/digest and target. A pending receipt, audit append alone,
matching trace or client claim is insufficient. Gauge/exemplar attribution is
rejected.

Confirmed-exposure bindings are outcome/objective evidence, not required
runtime input sources. Validation rejects a circular first-decision
prerequisite. Required input bindings use `attribution.kind: none`.

Decision and exposure snapshots retain caller values, resolved values and
per-input binding, generation, source timestamp/fingerprint, materialization
time, coverage, available trace/span/flags/exposure references, and resolved
evidence target with its resolution source/claim. The target remains recorded
even outside the state fallback chain. Confirmation cannot replace this vector;
retained decide retries do not re-read changed telemetry.

The current confirmation lookup is in-memory. Previously validated durable
frames, including their exact redelivery, retain provenance after restart;
new references to lost confirmations fail closed. #49 owns consolidation of
record/exposure ownership under Evidence Store. This slice does not claim a
general persisted observation log, separate Outcome table, or query service.

## Audit and explanation

Audit is the invariant that lifecycle and runtime outcomes are reconstructable,
not a standalone service. Explanation projects stored definition/target,
inputs, selected authority, constraints, fallback, value and timestamp. It
cannot invent missing provenance or reinterpret authority.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [OTel Ingestion](../design/otel-ingestion/README.md)
- [Evidence Store](../design/evidence-store/README.md)
- [Async Analysis Pipeline](../design/async-analysis/README.md)
- [Stock Collector integration](../../examples/otel-evidence/README.md)
