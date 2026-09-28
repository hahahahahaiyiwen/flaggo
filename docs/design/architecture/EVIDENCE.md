# Evidence and learning

## Purpose

Evidence supports asynchronous executable generation. It is not a synchronous
input to Runtime Evaluation.

The architecture distinguishes:

| Concept | Meaning |
| --- | --- |
| Runtime attribute | Explicit value supplied to evaluate the current request |
| Runtime decision | Result returned by the Decision Service |
| Exposure evidence | Flaggo-owned observation that the application applied a decision |
| Outcome evidence | User-declared observation associated with an applied exposure |
| Generation provenance | Record of which evidence and method produced a candidate executable |

Conflating these concepts would make runtime depend on delayed telemetry or
would treat a returned-but-unused decision as an exposure.

## Evidence flow

```text
Decision Service
  -> RuntimeDecision
  -> application applies result
  -> SDK creates exposure context
  -> SDK emits Flaggo exposure evidence
                                      \
                                       -> OpenTelemetry pipeline
                                      /
application activity
  -> SDK logical evidence binding
  -> outcome evidence
  -> evidence ingestion and correlation
  -> Evidence Store
  -> asynchronous learning analysis
  -> CandidateExecutable
  -> Contract Service validation and activation
```

The application's OpenTelemetry provider, processor, exporter, and Collector
pipeline remain the transport boundary. The SDK does not make the Decision
Service persist a decision session and does not require a synchronous
exposure-confirmation call.

## Exposure evidence

A successful `RuntimeDecision` identifies the exact `contractDigest`,
`executableDigest`, result, and evaluation source. It does not prove the
application used the result.

After application, the SDK creates or updates its exposure context and emits a
Flaggo-owned OpenTelemetry observation. That observation must make the
following semantic information available for later correlation:

- the SDK-managed exposure ID;
- authenticated application/environment scope as supplied by the ingestion
  boundary, not by decision attributes;
- contract and executable digests;
- the applied result and relevant evaluation provenance;
- exposure time; and
- contract attributes required by declared correlations.

This is a conceptual requirement, not a final OpenTelemetry attribute layout.
The exact signal type, instrumentation scope, field names, projection, and
batching behavior remain deferred.

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

## Correlation

Outcome evidence is associated with an applied decision using both:

1. the exact exposure ID; and
2. every contract attribute named by `correlateBy`.

The exposure ID identifies the applied decision. Correlation attributes bind
and validate the surrounding application activity. Neither replaces the
other, and neither is authentication data.

Conceptually:

```text
OutcomeObservation {
  exposureId,
  binding,
  value,
  correlationAttributes
}

ExposureObservation {
  exposureId,
  contractDigest,
  executableDigest,
  appliedResult,
  correlationAttributes
}

correlate(exposureId, required attributes)
  -> Evidence scoped to exact contractDigest
```

Missing or unknown exposure IDs, absent correlation attributes, mismatched
values, duplicate observations, and ambiguous matches must not be presented
as successful unambiguous correlation. Whether ingestion rejects, quarantines,
or retains such observations is part of the deferred evidence-correlation
failure design.

## Evidence storage boundary

The Evidence Store contains observations and correlation results needed by
asynchronous learning and generation provenance. Evidence remains associated
with the contract digest under which the exposure occurred.

The store is not:

- a request-time operand source for the Decision Service;
- a replacement for explicit `RuntimeInput`;
- a runtime target or authority store;
- proof that every returned decision was applied; or
- authorization data.

Retention, deduplication, late-arrival handling, aggregation, and physical
storage are evidence-service concerns. They do not alter Runtime Evaluation
semantics.

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

## OpenTelemetry profile boundary

The following protocol details are intentionally deferred:

- supported span, log, and metric representations;
- instrumentation scope and semantic-convention names;
- selector and projection syntax;
- event-time and ingestion-time semantics;
- multi-match and duplicate behavior;
- propagation and allocation of exposure IDs;
- required SDK and Collector transformations; and
- failure behavior for invalid or ambiguous correlation.

Until that profile is defined, the architecture establishes logical ownership
and correlation requirements but does not claim a portable wire mapping for
all OpenTelemetry backends.

## Invariants

1. Runtime Evaluation never waits for or queries evidence.
2. Only an applied result creates exposure evidence.
3. The SDK emits exposure evidence through OpenTelemetry rather than requiring
   server-side runtime session state.
4. Outcome evidence is declared as one logical observed value per contract
   entry.
5. Correlation uses both exact exposure identity and declared activity
   attributes.
6. Evidence remains scoped to the exact contract digest under which it was
   observed.
7. Learning produces an immutable candidate and cannot activate it directly.
8. Evidence delay or analysis failure leaves the existing runtime activation
   unchanged.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Decision authority](AUTHORITY.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
