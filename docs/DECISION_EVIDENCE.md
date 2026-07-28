# Decision Evidence

## Purpose

Decision evidence is what Flaggo knows from runtime facts, observations, telemetry, evidence views, quality signals, and provenance. It is one of Flaggo's three top-level mental-model components:

```text
Decision Definition
Decision Evidence
Decision Intelligence
```

Evidence informs decisions, but it does not create authority by itself. Authority comes from governance and governed state.

## Core idea

Decision evidence has two broad forms:

| Evidence form | Meaning | Example |
| --- | --- | --- |
| Runtime context | Target identifiers and non-signal request metadata. | `sessionId`, `deviceType` |
| Runtime signal inputs | Typed signal values supplied with a decision request. | `tetris.boardPressure = 0.82` |
| Durable observations | Facts observed over time. | `piece_placed`, `session_ended`, emitted metrics |

Flaggo projects these facts into evidence views.

```text
signal: tetris.recentPlacementTimeMs
  target: cohort:new_players
  window: 24h
  filters: {}
  quality: sufficient
```

## What decision evidence owns

| Part | Meaning |
| --- | --- |
| Runtime context | Current request facts and identifiers. |
| Raw observations | Events, metrics, traces, logs, spans, or domain records. |
| Signal definitions | Immutable keyed schemas for observed measures. |
| Evidence views | Signal key + target + window + filters + freshness/quality. |
| Inference inputs | Typed signal values used for online inference. Code-first SDKs may bind them inside `inference.inputs`; the wire request carries them separately from runtime context. |
| Decision records | Returned value plus inference input values, target, definition revision, and audit/correlation ID. |
| Exposure records | Client-confirmed application/rendering of a returned value, linked to a decision record. |
| Evidence quality | Freshness, sample size, confidence, missingness, conflict, drift. |
| Target identifiers | Runtime facts used to resolve target hierarchy levels. |
| Application/build provenance | Caller metadata used for audit, migration, drift detection, and operations. |

Application/build provenance should not affect personalization or target selection by default. It can become a governed evidence dimension only when explicitly declared by a decision definition.

## Signals and derived evidence views

Decision definitions reference signal handles by role. Tooling can derive an explicit allowed-signal set from objective, inference, evidence, and guardrail references:

```text
derived allowed signals:
  - tetris.piecePlaced
  - tetris.boardPressure
  - tetris.earlyLossRate
```

Evidence views are derived from immutable signal keys, target hierarchy, and time/window/filter needs:

```text
tetris.dropInterval@2
  signal: tetris.earlyLossRate
  target: cohort:new_players
  window: 24h
  filters: {}
```

This lets multiple decision definitions reuse compatible observed signals without sharing governed state or requiring separate evidence binding declarations.

## Declared metrics and inference inputs

Some values are useful both as durable evidence and as inference inputs:

```text
boardPressure
recentPlacementTimeMs
recoveryFailures
```

They should be declared as immutable keyed metrics first, then optionally selected as inference inputs:

| Concept | Meaning | Best for |
| --- | --- | --- |
| Declared metric | App-computed or pre-materialized signal with stable semantics. | Async learning, validation, evidence views. |
| Inference input | Declared metric supplied with the decision request. | Fast online inference without service-side hot-path aggregation. |
| Decision-record input | Inference input value captured when Flaggo returns a decision. | Auditing what Flaggo recommended under the exact request context. |
| Exposure-captured input | Inference input value copied when the client confirms the value was applied or rendered. | Learning what happened under the exact context of a rendered decision. |

For example:

```text
metric: tetris.boardPressure
  type: number
  source: app-emitted

inferenceInputs:
  - tetris.boardPressure
```

The emitted metric is normal telemetry:

```text
metric boardPressure = 0.82
```

The request-time inference input is captured in a decision record when Flaggo returns a value:

```text
decisionId: decision-123
definition: tetris.dropInterval@2
runtime target: session:game-456
returned value: 850
inference inputs:
  tetris.boardPressure = 0.82
  tetris.recentPlacementTimeMs = 1420
  tetris.recoveryFailures = 2
auditId: audit-789
```

The client should create or confirm an exposure only when it actually applies or renders the returned value:

```text
exposureId: exposure-456
decisionId: decision-123
applied value: 850
appliedAt: 2026-07-28T15:58:00Z
```

Later outcome events should correlate with exposures, not merely returned decisions. The metric, decision, and exposure records answer different learning questions:

| Question | Better source |
| --- | --- |
| How often do new players reach high board pressure, regardless of Flaggo decisions? | Emitted metric |
| What value did Flaggo recommend for this request? | Decision record |
| When the app actually applied `850ms` under high board pressure, did players recover? | Exposure-captured inference input |

Design rule:

> If online inference needs a value, it must be a declared metric selected as an inference input. A code-first SDK may bind the current pre-aggregated value directly inside `inference.inputs`; it must still serialize that value into the wire request's separate `inputs` array. The service should not aggregate it on the hot path.

## Evidence views

An evidence view is a target-specific and time-specific projection of evidence.

```text
EvidenceView
  signal: tetris.recentPlacementTimeMs
  target: session:game-456
  window: 2m
  filters: {}
```

```text
EvidenceView
  signal: tetris.recentPlacementTimeMs
  target: cohort:new_players
  window: 24h
  filters: {}
```

The same immutable signal key can serve async learning and online inference through different views:

| Path | Typical evidence view |
| --- | --- |
| Async learning | Cohort/global views over hours, days, or weeks. |
| Online inference | Session/user/request views over seconds or minutes. |
| Audit/explanation | Immutable snapshot references used by the proposal or decision. |

## Runtime context

Runtime context is evidence for the current request. It can contain:

```text
sessionId = game-456
userId = user-123
cohort = new_players
deviceType = mobile
```

Runtime signal inputs are separate from ordinary context:

```text
tetris.boardPressure = 0.82
tetris.recentPlacementTimeMs = 1420
tetris.recoveryFailures = 2
```

The target resolver uses these facts with the decision definition's target hierarchy:

```text
session:game-456 -> user:user-123 -> cohort:new_players -> global
```

## Reuse across definition revisions

Evidence can be reused across decision definition revisions when semantics match.

| Layer | Default reuse behavior |
| --- | --- |
| Raw observations | Reusable when immutable signal keys match. |
| Signal definitions | Reusable by immutable signal key. |
| Evidence views | Reusable when signal key, target, window, and filters match. |
| Governed state | Not reused automatically; it belongs to decision intelligence/governance. |

Example: `tetris.dropInterval@2` may add a new signal such as `tetris.recoveryFailures`. It can reuse historical `tetris.recentPlacementTimeMs` and `tetris.earlyLossRate` views, while the new signal warms up.

## OpenTelemetry relationship

OpenTelemetry should be a preferred transport and correlation model, not the only evidence shape.

| OTel signal | Evidence contribution |
| --- | --- |
| Metrics | Rates, averages, percentiles, counts, guardrails. |
| Traces | Request/workflow path and latency attribution. |
| Span events | Domain events attached to operations. |
| Logs | Structured domain records and debugging signals. |
| Baggage/context | Propagated target or correlation metadata. |

Flaggo can expose domain-friendly evidence concepts while remaining compatible with OpenTelemetry pipelines.

## Design rule

> Evidence is reusable knowledge. It informs decision intelligence, but it does not become runtime authority until governance produces governed state.
