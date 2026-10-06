# Evidence and learning

## Purpose

Evidence supports asynchronous executable generation. It is not a synchronous
input to Runtime Evaluation.

The architecture distinguishes:

| Concept | Meaning |
| --- | --- |
| Runtime attribute | Explicit value supplied to evaluate the current request |
| Runtime decision | Result returned by the Decision Service |
| Decision observation | Flaggo-owned observation that the SDK received a decision |
| Candidate evidence telemetry | Application logs, metrics, or traces that may contain contract-relevant evidence |
| Correlated evidence | Async-analysis output that links decisions and app telemetry as usable evidence |
| Generation provenance | Record of which evidence and method produced a candidate executable |

Conflating these concepts would make runtime depend on delayed telemetry or
would treat a returned-but-unused decision as clean learning evidence.

The OTel Ingestion, Evidence Materializer, and planned Async Analysis app
boundaries are canonical in
[Application boundaries and lifecycle](APP_BOUNDARIES.md).

## Evidence flow

```text
Decision Service -> RuntimeDecision -> SDK decision-received event --+
                                                                    |
application activity -> ordinary logs, metrics, and traces ---------+
                                                                    |
                                                                    v
  -> application-owned OTel providers
  -> optional OpenTelemetry Collector
  -> Flaggo OTLP Receiver
  -> bounded durable Raw OTLP Inbox
  -> versioned Evidence Materializer
  -> Evidence Store
  -> asynchronous learning analysis and correlation
  -> CandidateExecutable
  -> Contract Service validation and activation
```

The application's OpenTelemetry providers, processors, and exporters remain
the producer boundary. A Collector is optional. When present, its configuration
is intentionally coarse-grained: it may forward all telemetry to Flaggo or
route telemetry under configured service or instrumentation namespaces. It does
not need to understand decision contracts, evidence sources, or dynamic
contract selectors.

The receiver validates and durably enqueues complete OTLP export requests
without extracting authority or routing metadata. One request may contain
several Resources and authority scopes. The Evidence Materializer owns
decomposition, typed decoding, route construction, relevance selection,
deduplication, and the query-ready Evidence Store projection. Async analysis
owns semantic interpretation and correlation.

The SDK does not make the Decision Service persist a decision session and does
not require a synchronous exposure-confirmation call.

## Phase 4 OTLP signals

Phase 4 implements OTLP/HTTP endpoints for logs (`/v1/logs`), metrics
(`/v1/metrics`), and traces (`/v1/traces`). Each endpoint accepts standard
Protobuf JSON (`application/json`) and binary protobuf
(`application/x-protobuf`) with identity or gzip request compression. A
successful response means the complete valid request was durably appended to
the Raw OTLP Inbox. The receiver does not return partial success for item-level
semantic failures discovered later by the materializer.

Receiver validation and materializer decoding use the same OTLP codec.
Protobuf JSON accepts both lower-camel JSON field names and original protobuf
field names at every nesting level. Exact duplicate fields, duplicate aliases,
and ambiguous oneof members fail before acknowledgement; accepted requests
retain their exact decompressed bytes in the inbox.

Decision-received observations are a clear Flaggo-owned log event because they
describe a discrete runtime decision. App telemetry such as
`board_pressure_mean_5s`, `board_pressure_max_5s`, `current_level`, latency,
queue depth, and failure rate may be represented as metrics, logs, or span
attributes depending on how the application is instrumented. Flaggo must not
require those signals to be re-emitted through a Flaggo-specific outcome
abstraction before they can become candidate evidence.

An application does not need the Flaggo SDK to send telemetry to Flaggo OTel
Ingestion. It can emit standard OTLP logs, metrics, or traces directly through
any OpenTelemetry SDK or Collector. For Phase 4, authentication may be added
later; declared routing authority uses the `flaggo.tenant`,
`flaggo.application`, and `flaggo.environment` OTel Resource attributes. The
receiver verifies only transport and the signal-specific export-request
message; it neither validates nor extracts those attributes. Authority,
candidate records, timestamps, observation identities, and source keys are
derived asynchronously by the Evidence Materializer. All three `flaggo.*`
attributes are required non-empty strings; `service.name` and deployment
environment attributes are not authority fallbacks.

OTLP/HTTP senders may use no compression or standard gzip compression.
Ingestion supports both and enforces a configurable decompressed request limit,
defaulting to the OTLP-recommended 64 MiB rather than the smaller contract API
JSON limits.

## Decision observations

A successful `RuntimeDecision` identifies the exact `contractDigest`,
`executableDigest`, result, and evaluation source. It does not prove the
application used the result.

For the Phase 4 Flaggo OTLP logs mapping, `decide()` emits a Flaggo-owned
OpenTelemetry log event after a successful response:

```text
eventName = "flaggo.decision.received"
flaggo.signal = "decision.received"
```

That observation must make the following semantic information available for
later analysis:

- the SDK-generated decision ID;
- tenant/application/environment authority from OTLP resource attributes;
- contract name;
- contract and executable digests;
- the returned result JSON and result hash;
- evaluation source and optional rule name;
- observation time; and
- optional correlation attributes supplied by the surrounding app/request
  context.

This is a raw decision observation. It is not, by itself, proof that the app
applied the result and is not clean learning evidence.

The SDK-generated decision ID is correlation metadata, not the Evidence Store
record key. A stable observation identity is derived separately from the raw
OTLP envelope unless the event supplies an explicit observation ID. Trace
records use the pair `(traceId, spanId)` as identity; a span ID alone is not
globally sufficient.

The next logical evaluation may carry the prior applied exposure as
`RuntimeInput.currentExposure`. That value supplies application context only.
It is not proof that the newly returned decision will be exposed and it does
not select runtime authority.

## Evidence source declarations

Each entry in `DecisionContract.learning.evidence` declares one logical
observed value:

```yaml
evidence:
  - name: recovery_failure
    description: Recovery failures observed after a runtime decision.
    attribute: recovery_failures_5s
    correlateBy:
      - session_id
    source:
      kind: metric
      scope: tetris.engine
      name: tetris.recovery_failure
      metricKind: sum
      unit: "{failure}"
      correlation:
        session_id:
          location: signal
          attribute: tetris.session.id
```

The fields mean:

| Field | Meaning |
| --- | --- |
| `name` | Contract-local identity of one observed value |
| `attribute` | Contract attribute whose schema defines that value |
| `correlateBy` | Additional contract attributes required to associate the surrounding activity |
| `source` | The one exact application-owned OTel metric, log, span, or span event selected for this evidence |

Every source matches an exact, case-sensitive instrumentation-scope and signal
name. Metric sources also match metric kind and unit. Span-event sources match
their parent span name. Each source's `correlation` map must contain exactly the
contract attributes listed by `correlateBy`; each mapping identifies a
`resource`, `scope`, `signal`, or span-event `parentSpan` attribute.

The contract does not name a vendor table or database column. The receiver
enqueues all valid application telemetry before selection. The materializer
always recognizes built-in Flaggo protocol observations and separately applies
active materialization routes to ordinary application telemetry.

Contract Service exposes every current authority-bound contract at
`/v3/decision-contract-catalog/current`. The complete deployed contract,
including `AuthorityScope`, is covered by `contractDigest`. The materializer
compiles that catalog into:

```text
CurrentContracts
  (AuthorityScope, contractName) -> contractDigest

ActiveSourceCounts
  MaterializationRoute -> u32
```

Several contracts may keep the same route active. Candidate admission is an
expected `O(1)` route lookup and does not enumerate those contracts. Ordinary
telemetry carries no contract digest and is stored once even when several
contracts use its source.

The catalog response `ETag` is opaque conditional-fetch state. It is never an
observation, provenance, diagnostic, or checkpoint identity. The worker
durably caches each valid catalog directly. A failed or invalid refresh retains
the previous catalog. With no catalog, an empty route map still keeps
`flaggo.decision.received` acquisition independent of catalog availability.

Decoder, identity, projection, or routing-protocol version changes define a
new global forward-checkpoint namespace. Catalog changes do not reset or bypass
the checkpoint and never revisit completed inbox batches. Newly active routes
apply when pending or future batches are first processed.

## Correlation and analysis

Correlation is not an ingestion guarantee in Phase 4. The Evidence Store
persists selected materialized observations plus indexed metadata such as
telemetry type, resource-derived scope, signal/name, observation identity,
observation time, selector version, and available inbox provenance. The Async
Analysis Service decides whether a decision observation plus app telemetry is
sufficient, unambiguous, timely, and contract-relevant enough to become usable
learning evidence.

Analysis may use:

1. a direct `flaggo.decision.id` when the app provides it;
2. every contract attribute named by `correlateBy`;
3. contract name/digest and evidence source identity; and
4. analysis-owned time windows or other documented heuristics.

Direct decision identity, metric names, span attributes, resource attributes,
and correlation attributes bind and validate the surrounding application
activity. They are candidate evidence facts; later auth policy can restrict
who may write them, but Phase 4 ingestion does not make auth the evidence
scope authority.

Conceptually, analysis reads materialized telemetry and interprets it as:

```text
ApplicationObservation {
  decisionId?,
  sourceIdentity,
  completeSignalPayload,
  correlationAttributes
}

DecisionObservation {
  decisionId,
  contractDigest,
  executableDigest,
  result,
  correlationAttributes
}

analyze(decision observations, application observations, required attributes)
  -> Evidence scoped to exact contractDigest
```

Missing or unknown decision IDs, absent correlation attributes, mismatched
values, duplicate observations, and ambiguous matches must not be presented as
successful unambiguous evidence.

## Evidence storage boundary

The Raw OTLP Inbox and Evidence Store have different durability and query
contracts.

One inbox entry stores one complete decompressed export-request payload plus
signal type, wire encoding, media type, transport compression, profile version,
receipt time, byte length, and a SHA-256 integrity hash. It is immutable while
retained and bounded by explicit age and byte policies. It provides durable
work, crash recovery, and recent replay; it is not the long-lived evidence
query model.

The versioned materializer produces:

- one candidate per log record;
- one candidate per span and selectable nested span event; and
- one candidate per Gauge, Sum, Histogram, Exponential Histogram, or Summary
  data point.

It recursively decodes `AnyValue`, derives separate logical identities and
canonical content digests, performs route admission, deduplicates, and writes
query-ready observations. A materialized observation preserves its complete
authority and source key, resource and instrumentation-scope context, selected
signal payload, producing versions, and inbox provenance.

Exactly one Evidence Materializer is active per database. Its forward
checkpoint and catalog state are single-writer coordination; active-active
materializers and partitioned work claims are deferred scale-out concerns.
The checkpoint is global across materializer, decoder, identity, projection,
and routing versions. An upgraded materializer continues after the last
committed inbox batch instead of replaying retained batches. Producing-version
fields remain observation provenance; they do not create a separate evidence
identity or progress stream.
The first accepted candidate owns an observation and its origin provenance.
Later candidates with the same logical identity and content digest are ignored.
Different content for that identity, including content produced after a
materializer upgrade, is an invariant violation: the accepted observation
remains unchanged and usable, while the incoming candidate is rejected and
recorded as a conflict diagnostic.

Evidence Store observations remain durable after raw inbox payloads expire.
OTel Ingestion periodically invokes the inbox-owned, receipt-age retention
operation; receiver appends and materializer reads do not perform cleanup.
Retention is independent of materialization progress. Changed materialization
versions apply only to inbox batches after the global forward checkpoint;
analysis never silently scans the inbox as a fallback.
Evidence Materializer exposes operational progress through structured process
events. Startup and every committed batch page report the global forward
checkpoint, active producing versions, exact retained batches after that
checkpoint, oldest pending receipt age, newest persisted evidence timestamp,
and evidence freshness.
Fatal startup, inspection, or materialization failures emit a structured
failure event before process exit. Duplicate and conflict counts remain part
of each committed-page event, while durable conflict and diagnostic totals are
reported at startup.
Materialization does not persist eager
observation-to-contract associations. Analysis loads an exact immutable
contract and joins its evidence declarations to reusable observations by
`AuthorityScope + SourceKey` before evaluating predicates and correlation.

The store is not:

- a request-time operand source for the Decision Service;
- a replacement for explicit `RuntimeInput`;
- a runtime target or authority store;
- proof that every returned decision was applied; or
- authorization data.

Receiver request limits, inbox retention, materializer selection,
deduplication, late-arrival handling, analysis aggregation, and physical
storage do not alter Runtime Evaluation semantics.

## Learning loop

For a contract with `learning`:

```text
accepted-ready contract digest
  -> wait learning.policy.evaluate.interval
  -> read correlated evidence for current contract
  -> run one bounded asynchronous analysis
  -> produce no candidate, failure, or proposed executable rules
  -> validate candidate against exact contractDigest
  -> persist one immutable inactive Candidate

separate Contract Service activator
  -> apply auto-activation policy
  -> atomically activate if still current and eligible
```

Only one analysis cycle may claim a contract-name workspace at a time. Each
cycle binds one exact current digest, immutable evidence cutoff, and opaque
Evidence Store watermark. The next interval starts after the previous cycle
reaches a terminal outcome. Recoverable attempts resume the same cycle without
advancing cadence. Runtime continues using the existing active executable
throughout evidence delay, analysis, failure, Candidate persistence, and
activation attempts.

`auto-activation` means a separate Contract Service activator may attempt to
replace the activation mapping after Async Analysis persists a validated
inactive Candidate. Candidate persistence does not imply activation. Runtime
never scans the Evidence Store or Executable Store for the latest generated
artifact.

Async Analysis chooses aggregation windows, populations, primary objective
aggregation, and analysis method. It records those choices and the evidence
references in generation provenance rather than adding them to the initial
contract syntax.

When a newer digest becomes current, the older cycle receives a bounded
checkpoint period and becomes superseded. Candidate admission independently
rechecks current identity, so catalog-detection delay cannot persist a stale
proposal. Analysis results and generation provenance remain attached to the
digest that produced them. Immutable source observations may be reused only
when a later analysis explicitly selects and interprets them.

## Phase 4 Flaggo OTLP logs mapping

Phase 4 defines a Flaggo-specific OTLP logs mapping for ingestion. It is not a
portable OpenTelemetry semantic convention.

Supported built-in signal names:

| Event name | `flaggo.signal` | Meaning |
| --- | --- | --- |
| `flaggo.decision.received` | `decision.received` | SDK received a runtime decision |

Required decision attributes:

- `flaggo.decision.id`
- `flaggo.contract.name`
- `flaggo.contract.digest`
- `flaggo.executable.digest`
- `flaggo.result.json`
- `flaggo.result.hash`
- `flaggo.evaluation.source`
- optional `flaggo.evaluation.rule`

Correlation attributes use `flaggo.correlation.<name>`.

## Invariants

1. Runtime Evaluation never waits for or queries evidence.
2. `decide()` emits decision-received telemetry, not clean learning evidence.
3. The SDK emits telemetry through OpenTelemetry rather than requiring
   server-side runtime session state or confirmation RPCs.
4. Evidence is declared as one logical observed value and one required OTel
   source per contract entry.
5. Async analysis, not ingestion, decides whether decision observations and
   selected application telemetry are usable evidence.
6. Materialized application observations are reusable and contract-agnostic;
   analysis outputs and generation provenance remain scoped to the exact
   contract digest that interpreted them.
7. Learning produces an immutable candidate and cannot activate it directly.
8. Evidence delay or analysis failure leaves the existing runtime activation
   unchanged.

## Related documents

- [Application boundaries and lifecycle](APP_BOUNDARIES.md)
- [OTel Ingestion app](../../../apps/otel-ingestion/README.md)
- [Evidence Materializer app](../../../apps/evidence-materializer/README.md)
- [Architecture overview](OVERVIEW.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Decision authority](AUTHORITY.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
