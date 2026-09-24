# Decision evidence

## Boundary

Applications own OpenTelemetry instrumentation, providers, exporters,
Collectors, retention, and sampling. Flaggo binds selected existing telemetry
to decision-local meaning. It is not a telemetry producer schema or warehouse.
Evidence informs execution, policy, audit, and future proposals; it never
creates authority.

| Data | Owner and role |
| --- | --- |
| Context | Caller facts and identifiers, verified by runtime target resolution |
| Request input | Current typed operand supplied directly; no instrument is required |
| Input evidence | Declared scalar projected from received native OTel records |
| Policy-quality evidence | Separate optional quality/model contract, queried only by an explicit policy |
| Decision and exposure | Audit/state-owned operational records, not sampled telemetry |

```text
application OTel APIs/providers -> existing Collector pipelines
  -> authenticated OTLP ingress -> registered binding projection
  -> durable immutable input generation
  -> one runtime input resolution -> numeric rule + policy -> audit
```

## Supported projections

Every binding specifies exact scope name (optional version), resource and
record selectors, type/meaning, target, `latest` projection,
`freshness.maxAgeSeconds`, `sampling.accept: observed`, and attribution.

| Native source | Supported value | Source time |
| --- | --- | --- |
| Metric Gauge | One scalar data point, exact unit | Point timestamp |
| Span | Duration in milliseconds or primitive attribute | Span end |
| Named span event | Primitive event attribute | Event timestamp |
| Log | Primitive attribute or scalar at a structured body path | Log timestamp |

Logs match an event name or an exact scalar body. Body paths traverse native
structured values; JSON-looking strings are not parsed. Attribute namespaces
remain distinct. Missing fields, wrong types/units, no-recorded-value flags,
and out-of-range observations are not zeroes.

Sums/counters, histograms, quantiles, rates, rolling windows, arbitrary
queries, text extraction, and exemplar-based attribution are not supported.
An OTel span event or named log record needs no Flaggo event declaration.

## Freshness and ambiguity

Timestamps retain their original unsigned nanoseconds as decimal strings.
Freshness is inclusive: `0 <= evaluationTime - sourceTime <= maxAgeSeconds`.
Missing/invalid source times are not replaced with ingestion time. Future
observations and clock regression fail freshness. Duration must be a positive
integer no larger than `922337203685` seconds.

Exact redelivery is idempotent; out-of-order data cannot overwrite a newer
value or refresh its age. Conflicting values at the same latest timestamp
are ambiguous. Multiple fresh Gauge streams for one binding target are also
ambiguous: no averaging or last-writer-wins selection occurs. Narrow selectors
or target dimensions to select a single series. A newer identifiable invalid
observation invalidates the previous good value.

Span/log latest means the latest matching **received** record, not necessarily
the latest event that occurred in the application.

## Scope, materialization, and failure

Hosts derive tenant/application/environment from authenticated claims.
Resource attributes and client JSON cannot supply tenant authority. Frames
are partitioned by that scope, exact definition identity, binding, and target;
new revisions do not inherit older frames implicitly.

Ingestion pins approved, non-retired bindings. It validates and durably
publishes a bounded immutable generation before acknowledging accepted data.
Runtime reads pin one generation and one evaluation time; they do not scan
raw records, aggregate, wait for exports, or independently refresh each input.

The local store holds one writer lease and uses verified committed-file
snapshots. Restart verifies the committed generation. Corrupt or uncertain
publication fails closed until a verified reload. Capacity failures do not
evict fresh required inputs or acknowledge uncommitted work.

Missing, stale, future, ambiguous, invalid, or unavailable required evidence
returns `503 required-evidence-unavailable`, with a binding-specific reason
and `clientFallback.eligible: false`. There is no inferred default. A
request-only decision with no evidence-dependent policy reads neither evidence
port. Numeric inference reports `confidence: null`.

## Sampling and collection

Trace head sampling happens in the application SDK when a span starts;
Collector tail sampling, when configured by the application, selects after
receiving spans and may need trace affinity. Export batching, filtering,
buffer limits, and lost delivery can further reduce observations.

Metrics normally aggregate measurements into exported points and are not
controlled by trace sampling. Logs have their own filtering/delivery behavior;
retaining a log or a sampled span does not certify population completeness.
Collector pipeline placement determines whether Flaggo receives a filtered or
less-filtered branch. Keep existing backends instead of replacing them.

This slice accepts only observed coverage. Trace flags and available IDs are
provenance, not evidence of unbiased sampling, complete counts, statistical
confidence, or model quality.

## Exposure and outcomes

```text
apply returned value -> explicit confirmation -> committed exposure
  -> attach confirmed attributes to an existing span/log
  -> verified outcome binding
```

Confirmation is an operational write, never a sampled event. A binding that
requires confirmed attribution resolves `flaggo.exposure.id` through the
state-owned confirmed-exposure reader and checks authenticated scope, exact
definition identity, and the resolved target. Unused receipts, pending
confirmations, foreign identities, and mere trace/baggage correlation are not
proof of exposure. Gauge/exemplar attribution is rejected.

The audit and exposure snapshot preserve caller inputs, resolved inputs, and
per-input provenance: binding, generation, source time/fingerprint,
materialization time, observed coverage, trace/span IDs and flags where
available, verified exposure ID, and the resolved evidence target with its
resolution source and original claim. Evidence-target provenance is retained
even when state resolution uses a different target or fallback chain.
Confirmation cannot replace that vector.
Retained decide retries return the original result even after telemetry changes.

## Integration

See [telemetry/evidence design](../design/telemetry-evidence/README.md) for
transport limits, authorization, persistence, and recovery, and
[the stock Collector example](../../examples/otel-evidence/README.md) for a
complete cloud-free SDK-to-Collector-to-Flaggo flow.
