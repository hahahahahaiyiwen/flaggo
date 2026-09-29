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

## Evidence flow

```text
Decision Service
  -> RuntimeDecision
  -> SDK emits decision-received telemetry
                                      \
                                       -> OpenTelemetry pipeline / Collector
                                      /
application activity
  -> ordinary OpenTelemetry logs, metrics, and traces
  -> candidate evidence ingestion and raw observation storage
  -> Evidence Store
  -> asynchronous learning analysis and correlation
  -> CandidateExecutable
  -> Contract Service validation and activation
```

The application's OpenTelemetry provider, processor, exporter, and Collector
pipeline remain the transport boundary. Collector configuration is intentionally
coarse-grained: it may forward all telemetry to Flaggo ingestion, or forward
telemetry under configured service/instrumentation namespaces. Collector
configuration does not need to understand decision contracts, evidence
bindings, or dynamic contract-aware filters. Flaggo OTel Ingestion accepts
candidate telemetry, and async analysis owns contract-aware interpretation.

The SDK does not make the Decision Service persist a decision session and does
not require a synchronous exposure-confirmation call.

## Phase 4 OTLP signals

Phase 4 implements OTLP HTTP JSON endpoints for logs (`/v1/logs`), metrics
(`/v1/metrics`), and traces (`/v1/traces`). Decision-received observations are
a clear Flaggo-owned log event because they describe a discrete runtime
decision. App telemetry such as `board_pressure_mean_5s`,
`board_pressure_max_5s`, `current_level`, latency, queue depth, and failure
rate may be represented as metrics, logs, or span attributes depending on how
the application is instrumented. Flaggo must not require those signals to be
re-emitted through a Flaggo-specific outcome abstraction before they can become
candidate evidence.

`flaggo.outcome.observed` remains a convenience log shape for applications
that want to publish an explicit outcome value, but it is not the only outcome
model. Outcome is defined by the decision contract's learning/evidence
declarations and by async analysis over raw candidate telemetry, not by the
existence of a single SDK helper event.

An application does not need the Flaggo SDK to send telemetry to Flaggo OTel
Ingestion. It can emit standard OTLP logs, metrics, or traces directly through
any OpenTelemetry SDK or Collector. For Phase 4, authentication may be added
later; ingestion accepts telemetry with application/environment scope from OTLP
resource attributes such as `service.name`, `flaggo.application`,
`deployment.environment.name`, or `flaggo.environment`. Ingestion parses only
enough of the OTLP envelope to find resource scope, candidate records,
timestamps, stable observation IDs, and indexable signal names.

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
- application/environment scope from OTLP resource attributes;
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

## Outcome evidence declarations

Each entry in `DecisionContract.learning.evidence` declares one logical
observed value:

```yaml
evidence:
  - name: recovery_failure
    description: Recovery failures observed after a runtime decision.
    attribute: recovery_failures_5s
    binding: tetris.recovery_failure
    correlateBy:
      - session_id
```

The fields mean:

| Field | Meaning |
| --- | --- |
| `name` | Contract-local identity of one observed value |
| `attribute` | Contract attribute whose schema defines that value |
| `binding` | Logical SDK-to-OpenTelemetry binding name |
| `correlateBy` | Additional contract attributes required to associate the surrounding activity |

The contract does not name a vendor table, database column, metric instrument,
span path, or log field. One application activity may supply several logical
values, but the contract declares each value separately. Multiple declarations
may share an underlying activity through their SDK bindings.

## Outcome observations

Applications may emit telemetry that is not Flaggo-specific, as they would
without the Flaggo SDK. The SDK also provides convenience helpers that emit the
Flaggo OTLP logs mapping's explicit outcome log event:

```text
eventName = "flaggo.outcome.observed"
flaggo.signal = "outcome.observed"
```

The ingestion service stores ordinary application logs, metrics, and traces as
raw candidate evidence when they carry application/resource scope. Explicit
outcome observations may carry:

- `flaggo.evidence.binding`;
- `flaggo.evidence.value.json`;
- optional `flaggo.observation.id`;
- optional `flaggo.decision.id`;
- optional contract name/digest; and
- correlation attributes under `flaggo.correlation.<name>`.

## Correlation and analysis

Correlation is no longer an ingestion guarantee in Phase 4. The Evidence Store
persists accepted raw OTLP logs, metrics, and traces plus indexed metadata such
as telemetry type, resource-derived scope, signal/name, observation ID, and
observation time. The Async Analysis Service decides whether a decision
observation plus app telemetry is sufficient, unambiguous, timely, and
contract-relevant enough to become usable learning evidence.

Analysis may use:

1. a direct `flaggo.decision.id` when the app provides it;
2. every contract attribute named by `correlateBy`;
3. contract name/digest and evidence binding; and
4. analysis-owned time windows or other documented heuristics.

Direct decision identity, metric names, span attributes, resource attributes,
and correlation attributes bind and validate the surrounding application
activity. They are candidate evidence facts; later auth policy can restrict
who may write them, but Phase 4 ingestion does not make auth the evidence
scope authority.

Conceptually, analysis reads raw telemetry and interprets it as:

```text
OutcomeObservation {
  decisionId?,
  binding,
  value,
  correlationAttributes
}

DecisionObservation {
  decisionId,
  contractDigest,
  executableDigest,
  result,
  correlationAttributes
}

analyze(decision observations, outcome observations, required attributes)
  -> Evidence scoped to exact contractDigest
```

Missing or unknown decision IDs, absent correlation attributes, mismatched
values, duplicate observations, and ambiguous matches must not be presented as
successful unambiguous evidence.

## Evidence storage boundary

The Evidence Store contains accepted raw telemetry records needed by
asynchronous learning and generation provenance. Ingestion does not transform
OTLP logs, metrics, or traces into decision/outcome tables. It parses only
enough of the OTLP envelope to derive resource scope, telemetry type,
signal/name, idempotency key, and observation time for later analysis. Evidence
remains associated with the contract digest under which the decision occurred
when that identity is present in the raw telemetry payload.

Each stored record preserves a canonical raw envelope containing:

- the complete OTLP resource and resource schema URL;
- the complete instrumentation scope and scope schema URL; and
- the original log record, metric descriptor, or span.

Indexed metadata does not replace this envelope. It only supports bounded
selection and idempotent writes while later analysis retains access to service
instance, deployment, instrumentation, and schema context.

The store is not:

- a request-time operand source for the Decision Service;
- a replacement for explicit `RuntimeInput`;
- a runtime target or authority store;
- proof that every returned decision was applied; or
- authorization data.

Filtering, retention, deduplication, late-arrival handling, aggregation, and
physical storage are evidence-service concerns. They do not alter Runtime
Evaluation semantics.

## Learning loop

For a contract with `learning`:

```text
accepted-ready contract digest
  -> wait learning.policy.evaluate.interval
  -> read correlated evidence for current LearningHead
  -> run one bounded asynchronous analysis
  -> produce no candidate, failure, or CandidateExecutable
  -> validate candidate against exact contractDigest
  -> apply auto-activation policy
  -> atomically activate if still current and valid
```

Only one analysis run may be active for a contract digest. The next interval
starts after the previous attempt completes. Runtime continues using the
existing active executable throughout evidence delay, analysis, failure, and
activation attempts.

`auto-activation` means the Contract Service attempts to replace the
activation mapping after validation. Runtime never scans the Evidence Store or
Executable Store for the latest generated artifact.

The learning service chooses aggregation windows, populations, primary
objective aggregation, and analysis method. It records those choices and the
evidence references in generation provenance rather than adding them to the
initial contract syntax.

When a newer digest becomes `LearningHead`, an older run may finish for
reconstruction but cannot activate. Evidence observed for the older digest
remains attached to it; Flaggo does not silently reinterpret it as evidence
for the newer contract.

## Phase 4 Flaggo OTLP logs mapping

Phase 4 defines a Flaggo-specific OTLP logs mapping for ingestion. It is not a
portable OpenTelemetry semantic convention.

Supported signal names:

| Event name | `flaggo.signal` | Meaning |
| --- | --- | --- |
| `flaggo.decision.received` | `decision.received` | SDK received a runtime decision |
| `flaggo.outcome.observed` | `outcome.observed` | App/SDK reported a potentially relevant outcome |

Required decision attributes:

- `flaggo.decision.id`
- `flaggo.contract.name`
- `flaggo.contract.digest`
- `flaggo.executable.digest`
- `flaggo.result.json`
- `flaggo.result.hash`
- `flaggo.evaluation.source`
- optional `flaggo.evaluation.rule`

Required outcome attributes:

- `flaggo.evidence.binding`
- `flaggo.evidence.value.json`
- optional `flaggo.observation.id`
- optional `flaggo.decision.id`
- optional `flaggo.contract.name`
- optional `flaggo.contract.digest`

Correlation attributes use `flaggo.correlation.<name>`.

## Invariants

1. Runtime Evaluation never waits for or queries evidence.
2. `decide()` emits decision-received telemetry, not clean learning evidence.
3. The SDK emits telemetry through OpenTelemetry rather than requiring
   server-side runtime session state or confirmation RPCs.
4. Outcome evidence is declared as one logical observed value per contract
   entry.
5. Async analysis, not ingestion, decides whether decision and outcome
   observations are usable evidence.
6. Evidence remains scoped to the exact contract digest under which it was
   observed when that identity is known.
7. Learning produces an immutable candidate and cannot activate it directly.
8. Evidence delay or analysis failure leaves the existing runtime activation
   unchanged.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Decision authority](AUTHORITY.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
