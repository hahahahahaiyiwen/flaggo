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
| Outcome observation | Application telemetry that may be relevant to declared evidence |
| Correlated evidence | Async-analysis output that links decisions and outcomes as usable evidence |
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
  -> ordinary app telemetry or SDK outcome helper
  -> evidence ingestion and raw observation storage
  -> Evidence Store
  -> asynchronous learning analysis and correlation
  -> CandidateExecutable
  -> Contract Service validation and activation
```

The application's OpenTelemetry provider, processor, exporter, and Collector
pipeline remain the transport boundary. Collector configuration is intentionally
coarse-grained: a local profile may forward all telemetry to Flaggo ingestion,
or forward telemetry under configured service/instrumentation namespaces.
Collector configuration does not need to understand decision contracts,
evidence bindings, or dynamic contract-aware filters. Flaggo OTel Ingestion
always owns contract/profile filtering.

The SDK does not make the Decision Service persist a decision session and does
not require a synchronous exposure-confirmation call.

## Decision observations

A successful `RuntimeDecision` identifies the exact `contractDigest`,
`executableDigest`, result, and evaluation source. It does not prove the
application used the result.

For the Phase 4 local profile, `decide()` emits a Flaggo-owned OpenTelemetry
log event after a successful response:

```text
eventName = "flaggo.decision.received"
flaggo.signal = "decision.received"
```

That observation must make the following semantic information available for
later analysis:

- the SDK-generated decision ID;
- authenticated application/environment scope as supplied by the ingestion
  boundary, not by decision attributes;
- contract name;
- contract and executable digests;
- the returned result JSON and result hash;
- evaluation source and optional rule name;
- observation time; and
- optional correlation attributes supplied by the surrounding app/request
  context.

This is a raw decision observation. It is not, by itself, proof that the app
applied the result and is not clean learning evidence.

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
local profile's outcome log event:

```text
eventName = "flaggo.outcome.observed"
flaggo.signal = "outcome.observed"
```

The ingestion service accepts supported Flaggo outcome observations, ignores
non-Flaggo telemetry, and stores raw observations scoped by the authenticated
application/environment. Outcome observations may carry:

- `flaggo.evidence.binding`;
- `flaggo.evidence.value.json`;
- optional `flaggo.observation.id`;
- optional `flaggo.decision.id`;
- optional contract name/digest; and
- correlation attributes under `flaggo.correlation.<name>`.

## Correlation and analysis

Correlation is no longer an ingestion guarantee in Phase 4. The Evidence Store
persists raw decision and outcome observations. The Async Analysis Service
decides whether a decision observation plus outcome telemetry is sufficient,
unambiguous, timely, and contract-relevant enough to become usable learning
evidence.

Analysis may use:

1. a direct `flaggo.decision.id` when the app provides it;
2. every contract attribute named by `correlateBy`;
3. contract name/digest and evidence binding; and
4. analysis-owned time windows or other documented heuristics.

Direct decision identity and correlation attributes bind and validate the
surrounding application activity. Neither replaces authentication data.

Conceptually:

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

The Evidence Store contains raw decision and outcome observations needed by
asynchronous learning and generation provenance. Evidence remains associated
with the contract digest under which the decision occurred when that identity
is known.

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

## Local OpenTelemetry profile

Phase 4 defines a local OTel logs profile for Flaggo ingestion. It is not a
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
