# Flaggo Mental Model

## Purpose

Flaggo needs clear boundaries between the contract for a decision, the evidence used to reason about that decision, and the intelligence that turns evidence into governed behavior.

Detailed concept docs:

| Concept | Detailed doc |
| --- | --- |
| Decision Definition | [DECISION_DEFINITION.md](DECISION_DEFINITION.md) |
| Decision Evidence | [DECISION_EVIDENCE.md](DECISION_EVIDENCE.md) |
| Decision Intelligence | [DECISION_INTELLIGENCE.md](DECISION_INTELLIGENCE.md) |

The top-level model is:

```text
Decision Definition
  declares what may be decided and how targets/evidence/fallback resolve

Decision Evidence
  provides runtime facts, declared signals, evidence views, quality, and provenance

Decision Intelligence
  learns asynchronously and infers online within the definition and evidence

Decision Intelligence Output
  async: DecisionProposal -> governance -> GovernedDecisionState
  online: RuntimeDecisionResult, possibly containing fallback
```

`GovernedDecisionState` is not part of a decision definition. It is produced by async intelligence after governance approval and consumed by online inference. A runtime decision result is also not part of the definition; it is the output of online inference.

## Core concepts by layer

| Concept | Answers | Owns | Does not own |
| --- | --- | --- | --- |
| Decision key | What decision family does the application delegate? | Stable developer-facing name such as `tetris.dropInterval`. | Revision semantics, evidence history, active strategy. |
| Decision definition | What may be decided and how should the system resolve it? | Versioned contract: decision key, signals, intent, inference, output contract/action space, and safety constraints. | Raw telemetry history, application/build provenance, governed state, concrete runtime result. |
| Decision evidence | What is known now or historically? | Runtime facts, target identifiers, emitted events/metrics, evidence views, exposure records, evidence quality, uncertainty, provenance such as app/build identity. | Policy authority or active strategy state. |
| Decision intelligence | How should Flaggo learn or infer from the definition and evidence? | Async learning, online inference, proposal generation, strategy execution, reasoning mode selection. | Final authority without governance. |
| Governed decision state | What behavior has been approved for future/runtime use? | Active value, strategy, experiment, rollout, cooldown, override, lifecycle, previous safe value. | Decision definition semantics or raw evidence history. |
| Runtime decision result | What did this request receive? | Returned value, fallback status, explanation, audit ID, confidence, policy result. | Future authority unless persisted as governed state. |

## Decision definition

A decision definition is the contract for one semantic revision of a decision key. It should contain enough information to resolve targets and constrain intelligence, but it should not contain learned state or concrete runtime results.

```text
DecisionDefinition
  key: tetris.dropInterval
  revision: 2
  targetHierarchy: session -> user -> cohort -> global
  signals:
    allow:
      - tetris.boardPressure
      - tetris.recentPlacementTimeMs
      - tetris.piecePlaced
      - tetris.sessionEnded
      - tetris.earlyLossRate
      - tetris.hardDropRate
  intent:
      type: metric-objective
      primary:
        signal: tetris.earlyLossRate
        direction: minimize
      secondary:
        - signal: tetris.hardDropRate
          direction: target
          target: 0.45
      rationale: Keep the game challenging while reducing early frustration.
  inference:
      target: session
      inputs:
        - tetris.boardPressure
        - tetris.recentPlacementTimeMs
      fallbackOrder: cohort -> global
  output:
      type: number
      range: 200..1500
      step: 50
      default: 800
  requestedApproval: automatic
  safety:
      constraints:
        - type: max-step-change
          value: 50
        - type: cooldown
          duration: 20s
        - type: min-evidence-quality
          value: 0.70
        - type: max-model-uncertainty
          value: 0.35
        - type: min-sample-size
          value: 30
```

Notes:

- The decision key is a sub-concept of the decision definition: it identifies the decision family.
- The definition owns the output **contract** or action space, not the actual runtime result.
- Signal definitions are owned outside individual decisions, usually near the producer. A signal key such as `tetris.boardPressure` is the immutable semantic identity for its schema, type, units, range, and meaning.
- A decision definition does not redefine signal schemas. It explicitly allows the signal handles it may use and assigns them roles as objectives, inference inputs, evidence, or guardrails.
- `targetHierarchy` defines meaningful target levels for signal aggregation, evidence views, learning, inference, governance, and fallback.
- Derived signals must be declared separately from decisions and must state how they are derived from available signals.
- `inference.target` describes the desired online inference target kind, not a concrete target instance.
- `inference.inputs` identifies allowed app-emitted metrics and supplies their current pre-aggregated values. Code-first SDKs may express both through bound handles such as `boardPressureSignal.input(boardPressure)`; extracted definitions retain only the signal references, while runtime requests carry the values.
- `inference.fallbackOrder` makes broader fallback levels explicit.
- Intent is typed. Natural-language intent captures product direction; metric-objective intent binds optimization to declared signals.
- Natural-language intent is advisory metadata unless paired with metric objectives or typed policy constraints.
- Safety should use typed constraints when behavior must be machine-enforced. Labels such as `gradual` can remain presets only if they expand to concrete constraints.
- Application-authored definitions can request an approval mode, but deployment or environment policy grants authority. A definition cannot grant itself automatic approval.
- Evidence views are derived from the decision definition revision, referenced signal definitions, target hierarchy, and time/window needs; they do not need to be manually bound as a separate concept in the definition.

## Decision evidence

Decision evidence is the umbrella for facts, signals, and observations used by decision intelligence. It includes request-time facts and durable observations, but it does not create authority by itself.

```text
Runtime context:
  sessionId = game-456
  userId = user-123
  cohort = new_players
  boardPressure = 0.82
  recentPlacementTimeMs = 1420
  currentLevel = 3

Inference inputs used by online inference:
  boardPressure <- declared metric boardPressure
  recentPlacementTimeMs <- declared metric recentPlacementTimeMs

Runtime target resolved from context:
  session:game-456

Evidence view:
  signal: earlyLossRate / cohort:new_players / 24h

Application/build provenance:
  app=tetris-demo
  service=web
  build=tetris-web-2026-07-25.1
```

Application/build provenance supports audit, migration, drift detection, and operations. It should not affect personalization or target selection by default unless a definition explicitly binds it as an evidence dimension.

A value used online should be a declared signal, usually an app-emitted metric with a precomputed current value, and then explicitly selected as an inference input.

| Layer | Relationship to inference inputs |
| --- | --- |
| Decision definition | Declares signals and names which app-emitted metrics are inference inputs. |
| Decision evidence | Carries emitted metric values, request-time inference input values, exposure records, and historical evidence views. |
| Decision intelligence | Uses inference inputs for fast online inference and historical metric/event evidence for async learning. |

This keeps the top-level model small while avoiding arbitrary context fields. Context may carry values, but only declared inference inputs are meaningful to online inference.

## Decision intelligence

Decision intelligence has two explicit loops over the same definition and evidence model:

```text
Async learning loop:
  DecisionDefinition + DecisionEvidence + outcomes + typed intent/objectives
  -> DecisionProposal
  -> governance
  -> GovernedDecisionState

Online inference loop:
  DecisionDefinition + runtime context + compatible GovernedDecisionState + policy
  -> RuntimeDecisionResult, possibly containing fallback
```

Async learning may operate at broader targets than online inference. For example, it may learn from `cohort:new_players` or `global` evidence and produce governed state at `cohort:new_players`. Online inference can then apply that governed state to `session:game-456`.

## Scope vocabulary

The word "scope" should not carry every meaning. Decision definitions declare a signal target hierarchy, and resolvers choose path-specific targets from that hierarchy.

| Term | Meaning |
| --- | --- |
| Target hierarchy | Ordered target kinds where evidence, learning, control, inference, and fallback may resolve. |
| Runtime target | Concrete entity receiving this decision now. |
| Learning target | Target chosen by async intelligence for analysis. |
| Control target | Target where governed state is approved and stored. |
| Evidence view | Immutable signal key plus target, window, filters, freshness, and quality. |
| Policy scope | Boundary where a policy applies. |
| Fallback scope | Boundary where a fallback value or rule applies. |
| Application/build provenance | Software artifact identity used for audit and operations, not a personalization target by default. |

Target hierarchy is not a guarantee that every target kind forms a clean total order. Some target kinds, especially cohorts, can overlap. Resolution must therefore use selector semantics and precedence, not just "nearest ancestor wins."

Target resolution should define:

| Rule | Meaning |
| --- | --- |
| Selector | Predicate or identifier that determines whether a target applies. |
| Specificity | More specific targets usually outrank broader targets. |
| Priority | Explicit numeric or ordered priority breaks ties among overlapping targets. |
| Supersession | A state can replace another only through explicit `supersedesStateId`, a unique-active-state invariant, or policy-mediated conflict resolution. |
| Conflict result | If two applicable states cannot be ordered safely, policy should force fallback or operator review. |

Resolution may still produce chains such as:

```text
session:game-456 -> user:user-123 -> cohort:new_players -> global
```

But the chain is used differently by different layers:

- online runtime resolves a runtime target and applicable control state,
- async learning resolves a learning target with enough evidence,
- evidence resolves one or more evidence views,
- policy resolves applicable constraints,
- fallback resolves a safe value,
- audit records all resolved references.

`inference.fallbackOrder` is explicit, not implied by the hierarchy. If it says `cohort -> global`, skipping `user` is intentional for that decision revision; it means user-level governed state should not be used as fallback unless the definition includes it.

## Governance lifecycle, governed state, and runtime result

Use explicit names:

| Name | Meaning |
| --- | --- |
| `DecisionProposal` | Candidate value, strategy, experiment, hold, rollback, or fallback recommendation produced by intelligence. |
| `GovernedDecisionState` | Approved durable authority that online inference may consume. |
| `RuntimeDecisionResult` | Per-request response returned to application code. |

`DecisionProposal` and `GovernedDecisionState` have separate lifecycles:

```text
DecisionProposal:
  proposed -> validated -> approved | rejected

GovernedDecisionState:
  pending -> active -> superseded | expired | rolled-back
```

Validation checks schema compatibility, output bounds, typed safety constraints, target authority, evidence quality, and policy. Approval can be automatic or human-controlled only when authorized by deployment or environment policy:

| Approval mode | When appropriate |
| --- | --- |
| Automatic | Low-risk changes within typed constraints, sufficient evidence quality, model uncertainty limits, no conflicting active state, metric-objective intent, and environment policy grants automatic approval authority. |
| Human approval | New strategy classes, high-impact changes, insufficient evidence quality, excessive model uncertainty, weak expected outcome, overlapping target conflicts, policy exceptions, or regulated/business-critical decisions. |
| Operator override | Emergency pause, forced fallback, rollback, or manually pinned value. |

Effective policy is the intersection of definition constraints, environment policy, and operator controls. Less-trusted or narrower layers may restrict behavior but never widen it; an application-authored definition cannot override environment approval requirements, relax mandatory evidence-quality floors, or bypass an operator pause.

Active `GovernedDecisionState` is then consumed by online inference:

```text
runtime target + runtime context
  -> resolve applicable governed state
  -> execute active value, strategy, experiment, or fallback
  -> RuntimeDecisionResult
```

The `RuntimeDecisionResult` is an output record, not part of the definition:

```text
value: 850
fallbackUsed: false
decisionId: decision-123
confidence:
  evidenceQuality: 0.82
  modelUncertainty: 0.31
  expectedOutcome: 0.72
policy:
  result: approved
auditId: audit-789
```

Avoid treating confidence as one universal number. Operators need to know whether a score describes evidence quality, model uncertainty, or expected outcome. Policy has its own result and should not be duplicated inside confidence:

| Confidence field | Meaning |
| --- | --- |
| `evidenceQuality` | Freshness, sample size, missingness, and consistency of the evidence used. |
| `modelUncertainty` | Uncertainty in a learned model or strategy estimate; lower may be better depending on representation. |
| `expectedOutcome` | Estimated likelihood or magnitude of achieving the objective. |

## Closed-loop attribution

Adaptive learning needs explicit attribution. A runtime result should create a decision record. An exposure should be recorded only when the client applies or renders that returned value:

```text
RuntimeDecisionResult
  -> decisionId
  -> application applies or renders value
  -> confirmExposure(decisionId)
  -> client-applied exposureId
  -> outcome events within attribution window
  -> attributed evidence
  -> future DecisionProposal
```

Decision records should capture definition revision/hash, runtime target, resolved control target, governed state ID, returned value, fallback status, inference input values, policy result, audit ID, and timestamp. Exposure records should link to decision records and capture the fact that the application actually applied or rendered the value. Outcome events should declare attribution windows so unused responses, delayed outcomes, censoring, confounding, and selection bias can be handled explicitly rather than silently training the wrong lesson.

## Reuse rule

Different decision definitions should not automatically share active decision authority. They can reuse evidence when semantics match.

| Layer | Default reuse behavior |
| --- | --- |
| Raw telemetry observations | Reusable when event and field semantics match. |
| Evidence views/signals | Reusable only when signal definitions and aggregation semantics are compatible. |
| GovernedDecisionState | Isolated by decision definition revision/hash and control target unless explicitly declared compatible. |
| RuntimeDecisionResult, decision records, and confirmed exposures | Bound to the exact definition revision/hash used by the request. |

Runtime requests should bind to an expected decision definition revision or contract hash. `GovernedDecisionState` should declare which revisions or contract hashes it is compatible with. New definitions can start partially warm only through semantic compatibility: unchanged signals and evidence may be reused, while new or changed signals warm up before policy allows them to influence proposals or online inference.
