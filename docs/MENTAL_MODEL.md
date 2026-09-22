# Flaggo Mental Model

## Purpose

Flaggo is a policy-first decisioning control plane with a runtime decision provider. It needs clear boundaries between the contract for a decision, the evidence used to reason about that decision, the control-plane lifecycles that govern change, and the runtime mechanisms that execute approved behavior.

Detailed concept docs:

| Concept | Detailed doc |
| --- | --- |
| Decision Definition | [DECISION_DEFINITION.md](DECISION_DEFINITION.md) |
| Decision Evidence | [DECISION_EVIDENCE.md](DECISION_EVIDENCE.md) |
| Decision Intelligence | [DECISION_INTELLIGENCE.md](DECISION_INTELLIGENCE.md) |
| Decision Lifecycles | [DECISION_LIFECYCLES.md](DECISION_LIFECYCLES.md) |
| Runtime Decision Execution | [RUNTIME_DECISION_EXECUTION.md](RUNTIME_DECISION_EXECUTION.md) |

The top-level model is:

```text
Decision Definition
  declares what may be decided and how targets/evidence/fallback resolve

Decision Evidence
  provides runtime facts, declared signals, evidence views, quality, and provenance

Decision Intelligence
  optionally analyzes evidence and proposes bounded changes

Control-plane decision lifecycles
  bundle approval or proposal governance
  -> GovernedDecisionState

Runtime decision execution
  fixed resolution, strategy evaluation, variant assignment,
  rollout routing, override, or fallback
  -> RuntimeDecisionResult
```

`GovernedDecisionState` is not part of a decision definition. It is produced by an approved control-plane lifecycle and consumed by runtime decision execution. A runtime decision result is also not part of the definition; it is the per-request output of the runtime decision provider.

Flaggo supports two authority workflows:

```text
bundle-approved:
  definition bundle + initial authority candidate
  -> authenticated bundle approval
  -> governed state

proposal-managed:
  definition + evidence + current state
  -> decision intelligence or another authorized producer
  -> DecisionProposal -> governance
  -> governed state
```

Both workflows converge on the same governed-state and runtime execution path.
The bundle contains an initial authority candidate, not self-approved active
state.

Control-plane lifecycles and runtime execution operate at different timescales. A lifecycle decides whether an optimization, experiment, or rollout should exist and how it progresses. Runtime execution applies the resulting approved state consistently for each request.

## Core concepts by layer

| Concept | Answers | Owns | Does not own |
| --- | --- | --- | --- |
| Decision key | What decision family does the application delegate? | Stable developer-facing name such as `tetris.dropInterval`. | Revision semantics, evidence history, active strategy. |
| Decision definition | What may be decided and how should the system resolve it? | Versioned contract: decision key, signals, intent, inference, output contract/action space, safety constraints, and authority workflow. Bundle-approved definitions may declare an initial authority candidate. | Raw telemetry history, application/build provenance, approved governed state, concrete runtime result. |
| Decision evidence | What is known now or historically? | Runtime facts, target identifiers, emitted events/metrics, evidence views, exposure records, evidence quality, uncertainty, provenance such as app/build identity. | Policy authority or active strategy state. |
| Decision intelligence | What bounded behavior should Flaggo recommend from the definition and evidence? | Optional async analysis, proposal generation, and reasoning mode selection for proposal-managed authority. | Bundle approval, lifecycle authority, governance approval, or per-request execution. |
| Decision lifecycle | How does declared or proposed behavior become and remain active? | Bundle approval, proposal governance, activation, observation, conclusion, promotion, supersession, and rollback. | Per-request value selection. |
| Governed decision state | What behavior has been approved for future/runtime use? | Active value or strategy and activation lineage; experiments, rollouts, cooldown, override, and previous-safe-state data only under explicit lifecycle contracts. | Decision definition semantics or raw evidence history. |
| Runtime decision execution | How is approved behavior applied to this request? | Fixed-value resolution, strategy evaluation, deterministic variant assignment, rollout routing, override, and fallback. | Proposing or approving future behavior. |
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
      - tetris.earlyLossRate24h
      - tetris.hardDropRate24h
  intent:
      type: metric-objective
      primary:
        signal: tetris.earlyLossRate24h
        direction: minimize
      secondary:
        - signal: tetris.hardDropRate24h
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
  lifecycle:
      authorityMode: bundle-approved
      initialAuthority:
        controlTarget: cohort:new_players
        kind: numeric-rule
        rule:
          threshold: 0.55
          valueAtOrAbove: 850
          valueBelow: 750
          weightedInputs:
            - tetris.boardPressure: 0..1 * 0.45
            - tetris.recentPlacementTimeMs: 0..2000 * 0.25
            - tetris.recoveryFailures: 0..5 * 0.20
            - tetris.currentLevel: 0..20 * 0.10
        rationale: Initial deterministic Tetris behavior.
  safety:
      constraints:
        - type: max-step-change
          value: 50
```

Notes:

- The decision key is a sub-concept of the decision definition: it identifies the decision family.
- The definition owns the output **contract** or action space, not the actual runtime result.
- A definition must explicitly permit experimentation before governed state can activate an experiment. It owns the experiment safety envelope, such as eligible assignment target kinds, allowed values, traffic limits, exposure requirements, and applicable approval constraints.
- Active experiment identifiers, variants, allocation weights, assignment salt/version, lifecycle status, and promotion or rollback state belong to `GovernedDecisionState`, not the definition.
- Signal definitions are owned outside individual decisions, usually near the producer. A signal key such as `tetris.boardPressure` is the immutable semantic identity for its schema, type, units, range, and meaning.
- A decision definition does not redefine signal schemas. It explicitly allows the signal handles it may use and assigns them roles as objectives, inference inputs, evidence, or guardrails.
- `targetHierarchy` defines meaningful target levels for signal aggregation, evidence views, learning, inference, governance, and fallback.
- Derived signals must be declared separately from decisions and must state how they are derived from available signals. Their aggregation and fixed window are part of the immutable signal meaning, so a change from `24h` to `7d` requires a new key.
- `inference.target` describes the desired runtime execution target kind, not a concrete target instance.
- `inference.inputs` identifies allowed app-emitted metrics and supplies their current pre-aggregated values. Code-first SDKs may express both through bound handles such as `boardPressureSignal.input(boardPressure)`; extracted definitions retain only the signal references, while runtime requests carry the values.
- `inference.fallbackOrder` makes broader fallback levels explicit.
- Intent is typed. Natural-language intent captures product direction; metric-objective intent binds optimization to declared signals.
- Natural-language intent is advisory metadata unless paired with metric objectives or typed policy constraints.
- Safety should use typed constraints when behavior must be machine-enforced. Labels such as `gradual` can remain presets only if they expand to concrete constraints.
- A bundle-authored initial authority is only a candidate. An authenticated
  control-plane approval grants authority; the definition cannot approve
  itself.
- Evidence views are derived from the decision definition revision, referenced signal definitions, target hierarchy, and filter/window needs; they do not need to be manually bound as a separate concept in the definition. A view may select a window for a raw event or app-emitted metric, but must not override a fixed-window derived signal.

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

Inference inputs used by runtime strategy evaluation:
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
| Decision intelligence | Uses historical metric/event evidence and exposure-captured inference inputs for async learning and proposal generation. |
| Runtime decision execution | Uses current declared inference inputs for bounded request-time strategy evaluation. |

This keeps the top-level model small while avoiding arbitrary context fields. Context may carry values, but only declared inference inputs are meaningful to runtime strategy evaluation.

## Authority workflows

The bundle-approved workflow supplies a deterministic initial authority without
decision intelligence:

```text
definition + initial authority candidate
  -> bundle validation
  -> authenticated bundle approval
  -> expected-baseline activation
  -> GovernedDecisionState
```

The proposal-managed workflow adds asynchronous reasoning or another authorized
producer later:

```text
Adaptive optimization lifecycle:
  DecisionDefinition + DecisionEvidence + outcomes + typed intent/objectives
  -> DecisionProposal
  -> governance
  -> GovernedDecisionState

Experiment lifecycle:
  declared experiment permission + hypothesis + candidate variants
  -> policy validation and approval
  -> active experiment state
  -> attributed exposures and outcomes
  -> continue, promote, stop, or roll back

Progressive rollout lifecycle:
  selected value or strategy
  -> policy validation and approval
  -> staged allocation
  -> observe guardrails
  -> advance, pause, complete, or roll back
```

Proposals may be produced by decision intelligence, operators, or other authorized automation. Governance, rather than the proposal source, grants authority.

Bundle approval and proposal governance differ in how the candidate is
produced, not in how runtime consumes the resulting state.

Runtime decision execution consumes the approved state:

```text
DecisionDefinition + runtime context + compatible GovernedDecisionState + policy
  -> fixed resolution, strategy evaluation, deterministic variant assignment,
     rollout routing, override, or fallback
  -> RuntimeDecisionResult
```

Async learning may operate at broader targets than runtime execution. For example, it may learn from `cohort:new_players` or `global` evidence and produce governed state at `cohort:new_players`. Runtime execution can then apply that governed state to `session:game-456`.

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

For the Tetris definition whose explicit fallback order is
`cohort -> global`, resolution produces:

```text
session:game-456 -> cohort:new_players -> global
```

But the chain is used differently by different layers:

- online runtime resolves a runtime target and applicable control state,
- async learning resolves a learning target with enough evidence,
- evidence resolves one or more evidence views,
- policy resolves applicable constraints,
- fallback resolves a safe value,
- audit records all resolved references.

`inference.fallbackOrder` is explicit, not implied by the hierarchy. If it says `cohort -> global`, skipping `user` is intentional for that decision revision; it means user-level governed state should not be used as fallback unless the definition includes it.

## Control plane and data plane

Polari separates definition management from runtime evaluation:

| Plane | Owns |
| --- | --- |
| Control plane | Definition bundles, immutable revisions, policies, optimization/experiment/rollout lifecycles, governed state, and registration receipts. |
| Data plane | Fixed resolution, strategy evaluation, deterministic variant assignment, rollout routing, override/fallback execution, and exposure confirmation for exact registered identities. |

Application deployment is a third, developer-owned lifecycle. Code-first declarations generate control-plane artifacts. For MVP, trusted application/bootstrap startup validates/applies those artifacts and initializes the runtime binding before data-plane use. A runtime call must carry `definitionId + revision + contractDigest`.

```text
code-first declaration
  -> static extraction
  -> application deployment
  -> trusted startup control-plane registration
  -> authenticated bundle approval
  -> initial governed authority activation
  -> ready registration receipt
  -> data-plane decision
```

Missing, unknown, conflicting, or retired identity is a contract/configuration error and cannot become local fallback. A registered definition may still return governed server fallback when evidence, policy, or state prevents adaptation. SDK-local fallback is reserved for explicitly configured data-plane availability failures.

Startup registration is an MVP control-plane client experience, not an architectural merger. Future clients can publish manually, through CI/CD or GitOps, through an init/deployment hook, through registry-first tooling, or use startup verify-only. Data-plane decide never registers definitions.

## Governance lifecycle, governed state, and runtime result

Use explicit names:

| Name | Meaning |
| --- | --- |
| `DecisionProposal` | Candidate value, strategy, experiment, hold, rollback, or fallback recommendation produced by intelligence, an operator, authorized automation, or derived by the server from an approved bundle candidate. |
| `GovernedDecisionState` | Approved durable authority that runtime decision execution may consume. |
| `RuntimeDecisionResult` | Per-request response returned to application code. |

For experimentation, keep lifecycle and execution terminology distinct:

| Name | Plane | Meaning |
| --- | --- | --- |
| Experiment permission | Definition | Explicit opt-in and safety envelope within which experiments may be approved. |
| Experiment lifecycle | Control plane | Proposal, validation, approval, activation, observation, conclusion, promotion, and rollback. |
| Active experiment state | Governed state | Experiment ID, variants, weights, assignment unit, allocation version, salt, status, and lifecycle timestamps. |
| Variant assignment | Data plane | Deterministic selection of an approved variant for a stable assignment target. |

Assignment must be deterministic for the same declared assignment target across service replicas. Long-lived experiments should normally use a stable semantic target such as user, session, or tenant rather than the running application instance. Request-level assignment is valid only when reassignment between requests is intentional. A typical assignment input is:

```text
experimentId + allocationVersion + assignmentTargetKind + assignmentTargetId + salt
```

Bundle-derived authority and independently generated proposals have separate
entry paths:

```text
Bundle initial authority:
  declared -> approval-pending -> authorized -> activated

DecisionProposal:
  proposed -> validated -> approved | rejected

GovernedDecisionState:
  pending -> active -> superseded | expired | rolled-back
```

Bundle-approved validation checks the declared target, inference inputs,
numeric rule, action space, fallback, and applicable runtime policy; an
authenticated actor approves that exact snapshot. The approval modes below
apply to proposal-managed authority:

| Approval mode | When appropriate |
| --- | --- |
| Automatic | Low-risk changes within typed constraints, sufficient evidence quality, model uncertainty limits, no conflicting active state, metric-objective intent, and environment policy grants automatic approval authority. |
| Human approval | New strategy classes, high-impact changes, insufficient evidence quality, excessive model uncertainty, weak expected outcome, overlapping target conflicts, policy exceptions, or regulated/business-critical decisions. |
| Operator override | Emergency pause, forced fallback, rollback, or manually pinned value. |

Effective policy is the intersection of definition constraints, environment policy, and operator controls. Less-trusted or narrower layers may restrict behavior but never widen it; an application-authored definition cannot override environment approval requirements, relax mandatory evidence-quality floors, or bypass an operator pause.

Active `GovernedDecisionState` is then consumed by runtime decision execution:

```text
runtime target + runtime context
  -> resolve applicable governed state
  -> return an active fixed value
     or evaluate an active strategy
     or assign an active experiment variant
     or route an active rollout
     or apply an override or fallback
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

When variant assignment is used, the result must also identify the assignment:

```text
decisionMode: experiment
experimentId: batch-size-01
variantId: treatment
allocationVersion: 1
assignmentUnit: user
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

Decision records should capture definition revision/hash, runtime target, resolved control target, governed state ID, returned value, decision mode, fallback status, inference input values, policy result, audit ID, and timestamp. Experiment decisions must additionally capture experiment ID, variant ID, allocation version, and assignment unit. Exposure records should link to decision records and capture the fact that the application actually applied or rendered the value. Outcome events should declare attribution windows so unused responses, delayed outcomes, censoring, confounding, and selection bias can be handled explicitly rather than silently training the wrong lesson.

## Reuse rule

Different decision definitions should not automatically share active decision authority. They can reuse evidence when semantics match.

| Layer | Default reuse behavior |
| --- | --- |
| Raw telemetry observations | Reusable when event and field semantics match. |
| Evidence views/signals | Reusable only when signal definitions and aggregation semantics are compatible. |
| Authority head | Stable by application, environment, decision key, and control target so every semantic revision participates in one ordered CAS lineage. |
| GovernedDecisionState | Immutable and bound to the exact definition revision/hash that was approved. |
| RuntimeDecisionResult, decision records, and confirmed exposures | Bound to the exact definition revision/hash used by the request. |

Runtime requests should bind to an expected decision definition revision or
contract hash. The active authority head may point to only one immutable state;
runtime uses it only when that state matches the request's exact definition
identity. An older registered request receives server fallback after a newer
revision replaces the head rather than consuming the new strategy. New
definitions can start partially warm only through semantic compatibility:
unchanged signals and evidence may be reused, while new or changed signals warm
up before policy allows them to influence proposals or runtime execution.
