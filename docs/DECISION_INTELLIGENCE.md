# Decision Intelligence

## Purpose

Decision intelligence is the AI-native reasoning layer that operates over decision definitions and decision evidence. For real-time adaptive decisions, async intelligence often produces a **decision strategy proposal**, while online inference consumes governed state to return one immediate value.

It sits between the declarative parts of the system and the governed runtime result:

```text
decision definition
  + decision evidence
  -> decision intelligence

async path:
  -> proposal
  -> governance
  -> governed state

online path:
  -> consume governed state when available
  -> runtime decision result, possibly containing fallback
```

This layer is what makes Flaggo different from a smart feature flag service. A feature flag service usually answers whether a configured value is enabled. Flaggo should reason about which safe value, behavior, or bounded adaptation strategy best serves the declared goal under current evidence, uncertainty, policy, and scope.

## Core idea

Decision intelligence should behave like an embedded data scientist and operator assistant for runtime decisions.

For a decision key such as `tetris.dropInterval`, a human data scientist might:

1. Understand the product goal.
2. Inspect available telemetry.
3. Decide whether the evidence is sufficient.
4. Choose an analysis strategy.
5. Design or interpret an experiment.
6. Compare candidate values or adaptation strategies.
7. Estimate expected impact and risk.
8. Recommend a value, strategy, experiment, hold, rollback, or fallback.
9. Monitor the outcome and revise the recommendation or strategy.

Flaggo should make that reasoning process explicit, automatable, governable, and auditable.

## Decision proposal, not final authority

Decision intelligence produces a **decision proposal**. It does not directly create the final runtime decision.

For simple decisions, the proposal may be a single value. For real-time adaptive decisions, the proposal should usually be a **decision strategy** that the online runtime path can execute quickly against live context and fresh telemetry.

A proposal may recommend:

- continue the current value,
- change to a new value,
- activate or revise a decision strategy,
- start or adjust an experiment,
- assign traffic among candidate values,
- roll back to a previous value,
- hold because evidence is insufficient,
- recommend fallback-only governed state because no adaptive decision is safe.

The proposal must then pass through governance:

```text
decision proposal
  -> policy constraints
  -> target authority
  -> lifecycle state
  -> cooldown and rollout limits
  -> operator overrides or approval requirements
  -> GovernedDecisionState or no state
```

This separation keeps the system AI-native without letting AI bypass safety controls.

## Decision strategies

A decision strategy is a governed plan for producing fast runtime decisions within a declared action space.

Instead of asking async intelligence to choose one value such as `900ms`, Flaggo can ask it to propose a bounded strategy:

```text
base value:
  800ms

allowed range:
  600ms to 1100ms

online adjustment rules:
  slow down when board pressure is high and placement is slow
  speed up when the player is stable and placement is fast

step and cooldown:
  change by at most 50ms at a time
  change at most once every 20 seconds

fallback:
  800ms
```

The online runtime path can then apply that approved strategy to current request context:

```text
current board pressure = high
recent placement time = slow
current interval = 800ms
approved strategy says slow down by 50ms

runtime decision = 850ms
```

This pattern is important for real-time use cases. It keeps online decisions fast while still letting async intelligence perform deeper analysis and strategy design.

Strategy proposals may take several forms:

| Strategy form | Online runtime behavior |
| --- | --- |
| Fixed value | Return one active governed value. |
| Rule table | Apply approved conditions and thresholds to current context. |
| Scoring function | Score candidates within the action space and return the best allowed value. |
| Small model | Run bounded inference over current context and declared precomputed metrics. |
| Bandit policy | Allocate among candidates according to approved exploration/exploitation rules. |
| Experiment assignment | Return the assigned candidate for the request's scope or exposure bucket. |
| LLM-backed strategy | Use bounded AI reasoning only where latency and policy allow it. |

Every strategy must still be governed by the declared result type, action space, fallback, policy, target authority, lifecycle state, and audit requirements.

## Inputs

Decision intelligence operates on resolved inputs, not raw unbounded prompts.

Core inputs include:

- **Decision definition**: the versioned semantic contract being reasoned about.
- **Control target**: the boundary where a proposal would apply.
- **Runtime target**: the entity receiving a runtime decision when the online path is involved.
- **Runtime context**: request-time facts supplied by the application.
- **Inference inputs**: declared metrics supplied with the decision request for fast online inference.
- **Evidence views**: observed behavior, metrics, traces, logs, and derived evidence snapshots sliced by target and time.
- **Goals**: the desired outcome or optimization intent.
- **Policy constraints**: known safety, compliance, and operating limits.
- **Governed decision state**: active value, previous values, cooldowns, overrides, rollout state, and historical proposals.
- **Uncertainty**: confidence, evidence quality, sample size, freshness, variance, and missing data.
- **Action space**: valid boolean, number, or string values and their constraints.
- **Decision strategy**: optional approved rules, thresholds, model, candidate set, or assignment plan used to make fast runtime decisions.
- **Fallback contract**: the safe value to return when no `GovernedDecisionState` or safe `RuntimeDecisionResult` can be made.

The agentic reasoning process should be constrained by these contracts. It should not invent new action types, ignore target boundaries, or exceed declared bounds.

## Runtime hierarchy

Decision intelligence participates in two timing paths:

```text
Async intelligence path
  -> may run agentic decision loop
  -> produces value proposal or strategy proposal
  -> governance stage
  -> active governed value, strategy, experiment, hold, or fallback-only state

Online runtime path
  -> serves application request
  -> uses active governed value, approved strategy, experiment, fallback, or case-specific runtime reasoning
  -> governance stage
  -> response to application
```

The async and online paths are comparable execution paths. Governance is not a peer path. Governance is a downstream control stage that proposals and runtime responses pass through before they can affect application behavior.

Agentic reasoning is expected to be most common in the async intelligence path, but it is not exclusive to that path. Some online decisions may use lightweight agentic reasoning when latency, cost, safety, and policy allow it. Governance may also use agentic assistance to explain, review, or recommend policy outcomes, but policy authority must remain explicit and enforceable.

## Async intelligence path

The async intelligence path performs deeper analysis outside the application's request/response critical path.

```text
Telemetry changes, schedule, operator request, definition activation, rollout review, or decision drift
  -> create intelligence work item
  -> decision definition + control target + trigger
  -> observe evidence views and governed state
  -> interpret findings
  -> choose analysis mode
  -> generate and evaluate candidate values or strategies
  -> produce proposal
  -> governance stage
```

This path is where Flaggo can behave most like an AI-assisted data scientist. It can spend more time comparing history, evaluating experiments, synthesizing evidence, and explaining recommendations. For inference inputs, it should learn from the corresponding declared metrics and from exposure-captured input values used by prior decisions.

The async path is the primary home of the agentic decision loop because it can tolerate longer-running reasoning, tool use, multi-step analysis, and proposal generation. For real-time adaptive surfaces, its most useful deliverable is often an approved strategy that online runtime can execute quickly, not a single initial value.

## Agentic decision loop

Decision intelligence can be modeled as an agentic loop:

```text
observe
  -> interpret
  -> choose analysis mode
  -> generate candidate values or strategies
  -> evaluate candidates
  -> produce proposal
  -> monitor outcome
  -> learn/update future reasoning
```

### 1. Observe

Collect the current evidence views for the decision definition and target.

Observation includes:

- current active value,
- recent outcome telemetry,
- evidence inherited from broader targets,
- prior decisions and proposal history,
- active experiments or rollouts,
- policy-relevant state.

### 2. Interpret

Translate evidence into decision-relevant findings.

Examples:

- users at level 1 are losing too quickly,
- hard-drop rate is low but placement time is high,
- evidence is fresh but sample size is too small,
- segment-level evidence is reliable while user-level evidence is sparse,
- a prior value change improved engagement but increased early abandonment.

### 3. Choose analysis mode

The system should choose an analysis mode based on available evidence, risk, and decision maturity.

Possible modes:

| Mode | When useful | Example |
| --- | --- | --- |
| Qualitative reasoning | Evidence is sparse or early; explainable recommendation is more important than optimization | Hold current value because telemetry is inconclusive |
| Heuristic rules | Domain rules are simple and stable | Increase interval when early game-over rate exceeds threshold |
| Experiment analysis | A/B or multivariate tests are active | Compare `800ms` vs `900ms` for new players |
| Bandit optimization | Continuous allocation among known candidates is appropriate | Shift traffic toward the interval with better retention |
| Statistical model | There is enough historical data to predict outcomes | Estimate game-over risk from current level and placement speed |
| LLM-assisted reasoning | Goals, evidence, and explanations require synthesis | Summarize tradeoffs and recommend whether more testing is needed |
| Hybrid | Multiple methods are needed | Use model scores, then have an LLM explain the proposal within policy context |

Not every decision needs an LLM. AI-native means the system can reason, choose tools, synthesize evidence, and propose actions; it does not mean every request must call a language model.

### 4. Generate candidates

Produce candidate actions or strategies within the declared action space.

For `tetris.dropInterval`, candidates might be:

- keep `800ms`,
- move to `850ms`,
- move to `900ms`,
- activate a rule strategy that adjusts between `600ms` and `1100ms` based on live board pressure and placement speed,
- start an experiment with `800ms`, `850ms`, and `900ms`,
- fall back to default because evidence is unsafe.

Candidates outside the contract, such as `50ms` or a structured object, should not be generated.

### 5. Evaluate candidates

Compare candidates against goals, evidence, uncertainty, and known risks.

Evaluation should consider:

- expected impact,
- confidence by meaning, such as evidence quality, model uncertainty, expected outcome, and policy eligibility,
- evidence quality,
- blast radius,
- reversibility,
- freshness of telemetry,
- scope fit,
- user experience risk,
- policy constraints likely to block the proposal.

### 6. Produce proposal

The output of the loop is a proposal that can be governed.

Example shape:

```json
{
  "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
  "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
  "runtimeTarget": "session:game-456",
  "controlTarget": "cohort:new_players",
  "proposalType": "activate_strategy",
  "currentValue": 800,
  "strategy": {
    "baseValue": 800,
    "allowedRange": {
      "min": 600,
      "max": 1100
    },
    "step": 50,
    "cooldown": "20s",
    "rules": [
      {
        "when": "boardPressure >= 0.70 && recentPlacementTimeMs >= 1200",
        "adjustBy": 50
      },
      {
        "when": "boardPressure == low && recentPlacementTime == fast",
        "adjustBy": -50
      }
    ]
  },
  "expectedImpact": "Adapt drop speed during a session so new players get recovery time under pressure without making stable play too slow",
  "confidence": {
    "evidenceQuality": 0.82,
    "modelUncertainty": 0.31,
    "expectedOutcome": 0.72
  },
  "evidenceStatus": "sufficient_for_control_target",
  "analysisMode": "qualitative_plus_metric_threshold_strategy",
  "alternativesConsidered": [
    "fixed_800ms",
    "fixed_900ms",
    "adaptive_600ms_to_1100ms"
  ],
  "rationale": "New-player evidence shows elevated early game-over rate and slow placement recovery, but one fixed value cannot respond to live game pressure. Segment-level evidence supports a bounded adaptive strategy for new-player sessions.",
  "risks": [
    "May make interval changes noticeable if cooldown and max delta are too loose"
  ]
}
```

This proposal is not yet the application response. It still needs policy and state governance.

### 7. Monitor outcome

After a `RuntimeDecisionResult` is applied, Flaggo should observe the result and feed it back into future reasoning.

Monitoring closes the loop:

```text
DecisionProposal -> GovernedDecisionState -> RuntimeDecisionResult -> decision record -> confirmed exposure -> outcome telemetry -> future DecisionProposal
```

Without outcome monitoring, Flaggo would only be making one-off recommendations.

## Online runtime path

The online path serves application requests and should remain fast, bounded, and safe.

```text
Application
  -> Decision API
  -> identify decision definition and runtime target
  -> load active governed state
  -> resolve approved experiment or rollout assignment if applicable
  -> fetch fresh-enough evidence if needed and allowed
  -> execute active governed value, approved strategy, approved experiment, or fallback-only state
  -> governance stage
  -> return governed value or fallback
```

Most online requests should not run the full agentic loop. They should usually consume state created by prior async intelligence and governance: active values, approved strategies, approved experiments, rollout assignments, operator overrides, and fallback contracts.

For the MVP, online reasoning without compatible `GovernedDecisionState` is prohibited. The online path must not invent a new value or strategy from request context alone.

Future online reasoning can be introduced only as an explicit **ephemeral runtime candidate** model:

```text
runtime context + compatible governed permission state
  -> bounded ephemeral candidate
  -> governance and policy
  -> RuntimeDecisionResult
```

The compatible governed state must authorize the candidate generator, action space, target authority, fallback behavior, audit requirements, and latency budget. If governance rejects the candidate, the response remains a normal `RuntimeDecisionResult` with fallback.

For real-time adaptive scenarios, the common pattern should be:

```text
Async intelligence designs strategy
  -> governance approves strategy bounds
  -> online runtime executes strategy against live context
  -> governance checks runtime value
  -> application receives immediate decision
```

## Governance stage

Governance turns proposals or runtime candidates into safe runtime authority. It is a downstream stage, not a third path parallel to online and async execution.

```text
Decision proposal, strategy proposal, or runtime candidate
  -> policy evaluation
  -> target authority check
  -> lifecycle and cooldown check
  -> approval or override handling
  -> governed state update, governed response, hold, rollback transition, or no state
```

Governance may approve a proposal as-is, constrain it, require human approval, reject it, activate fallback-only governed state, activate a bounded strategy, or allow a bounded runtime response. A rollback proposal is a transition: it activates a replacement or previous known-safe state and marks the replaced state as rolled back.

Governance may itself use agentic assistance for analysis or explanation. For example, an agent may summarize why a proposal appears risky or recommend which policy reason code applies. That assistance must not replace explicit policy enforcement. The final authority should remain inspectable as effective policy, runtime/control/evidence target references, lifecycle state, and operator controls.

## Supporting lifecycle flows

Online runtime and async intelligence are the primary execution paths. Governance is the downstream control stage. The system also needs supporting lifecycle flows that keep decisions declared, evidenced, governed, observable, and improvable over time.

These flows are not peers of the online and async paths. They do not directly serve application decision requests or produce deep analysis proposals. Instead, they prepare, feed, constrain, inspect, and learn from those paths.

```text
Primary execution paths:
  online runtime path
  async intelligence path

Downstream control stage:
  governance stage

Supporting lifecycle flows:
  contract sync
  telemetry ingestion
  operator intervention
  experiment lifecycle
  audit and explanation
  feedback and learning
```

### Contract sync flow

The contract sync flow keeps Flaggo's registry aligned with what application code declares.

```text
Developer, platform, or registry declares decision keys, target hierarchy, signals, policies, and fallbacks
  -> SDK extractor, manifest authoring, or registry export produces DecisionDefinitionBundle
  -> CI, release, GitOps, or operator workflow validates/applies bundle
  -> registry records versioned definition changes
  -> runtime and intelligence paths consume approved definitions
```

This flow should happen before production runtime whenever possible. Runtime application code should not silently create or mutate production decision contracts.

The contract sync flow answers:

- what decisions exist,
- who owns them,
- what values are valid,
- what fallback is safe,
- which telemetry and goals matter,
- which targets and policies apply,
- which contract version a decision used.

### Telemetry ingestion flow

The telemetry ingestion flow turns application observations into evidence that online runtime and async intelligence can use.

```text
Application emits domain telemetry or OpenTelemetry signals
  -> ingestion pipeline receives events, metrics, traces, or logs
  -> evidence service correlates and aggregates observations
  -> evidence snapshots become available by signal key, target, window, and filters
  -> online and async paths use evidence snapshots
```

This flow should preserve enough lineage for audit and explanation. A decision should be able to point back to the evidence snapshot, freshness, target, window, and quality that influenced it.

The telemetry ingestion flow answers:

- what happened after prior decisions,
- whether evidence is fresh,
- whether the sample is large enough,
- which scope has reliable evidence,
- whether outcomes improved or regressed.

### Operator intervention flow

The operator intervention flow lets humans control the system when judgment, risk, or accountability requires it.

```text
Operator reviews surface, evidence, proposal, or active state
  -> pauses, resumes, overrides, approves, rejects, or rolls back
  -> governance stage records the intervention
  -> runtime state reflects the operator decision
  -> audit trail captures who changed what and why
```

This flow is central to making Flaggo governed rather than magical. Operator controls should be explicit state, not hidden exceptions.

The operator intervention flow answers:

- should this decision be paused,
- should this proposal require approval,
- should the active value be overridden,
- should a bad rollout be rolled back,
- who made the intervention and why.

### Experiment lifecycle flow

The experiment lifecycle flow manages decisions that need controlled exposure before promotion.

```text
Decision intelligence or operator proposes experiment
  -> governance approves experiment bounds
  -> runtime assigns exposure
  -> telemetry ingestion records outcomes
  -> async intelligence analyzes results
  -> governance promotes, adjusts, stops, holds, or rolls back
```

Experiments are one strategy within Flaggo, not the whole system. They are useful when several candidate values are plausible and the safest answer is to learn under controlled constraints.

The experiment lifecycle flow answers:

- which candidates should be compared,
- how exposure should be split,
- what success metrics determine outcome,
- when evidence is sufficient,
- whether to promote, continue, stop, or roll back.

### Audit and explanation flow

The audit and explanation flow records why decisions, proposals, and governance outcomes happened.

```text
Decision request, proposal, policy result, operator action, or state change
  -> audit service records structured event
  -> explanation service stores human-readable rationale
  -> operator console exposes traceable history
  -> future analysis can reconstruct the decision
```

Audit should cover both online runtime decisions and async intelligence proposals. It should also include governance outcomes, fallback usage, scope resolution, evidence references, and operator interventions.

The audit and explanation flow answers:

- what decision was returned,
- what proposal led to a state change,
- which evidence and scope were used,
- which policy allowed or blocked the outcome,
- whether fallback was used,
- how to reconstruct the decision later.

### Feedback and learning flow

The feedback and learning flow closes the loop from runtime outcomes back into future reasoning.

```text
Governed decision is applied
  -> application emits outcome telemetry
  -> evidence snapshots update
  -> async intelligence detects changes or drift
  -> future proposals improve based on observed outcomes
```

This flow prevents Flaggo from becoming a one-shot recommendation system. Decisions should create evidence that improves later decisions.

The feedback and learning flow answers:

- did the decision improve the intended outcome,
- did it introduce regressions,
- should the current value remain active,
- should a new proposal be generated,
- should a model, heuristic, or experiment design be updated.

## Proposal outcomes

A proposal can resolve to several governed outcomes:

| Outcome | Meaning |
| --- | --- |
| `approved` | Proposal can become active for the target scope. |
| `limited` | Proposal is allowed only with reduced blast radius, smaller delta, narrower scope, or slower rollout. |
| `experiment` | Proposal should run as an experiment before becoming the default. |
| `hold` | Current state remains active because evidence is insufficient or risk is too high. |
| `rollback` | Transition to a replacement or previous known-safe state and mark the replaced state rolled back. |
| `fallback` | Activate fallback-only governed state, or produce no state if even fallback-only authority is not durable. |
| `requires-approval` | Human approval is needed before activation. |
| `rejected` | Proposal violates policy or target authority. |

## Relationship to policy

Policy is downstream of decision intelligence and has final authority.

Decision intelligence may understand policy context and avoid obviously invalid proposals, but policy evaluation must still independently enforce:

- min/max bounds,
- allowed string values,
- evidence quality and model uncertainty limits,
- sample-size requirements,
- cooldown windows,
- maximum deltas,
- rollout limits,
- target authority,
- approval requirements,
- operator pauses and overrides.

This prevents the reasoning layer from becoming an implicit policy engine.

## Relationship to targets

Targets affect both reasoning and governance.

Decision intelligence may propose at a different control target than the runtime target if evidence supports that boundary better.

Example:

```text
runtimeTarget = user:123
evidence is too sparse at user target
cohort evidence is sufficient for cohort:new_players
controlTarget = cohort:new_players
```

That is a target resolution choice, not a failed decision. Governance must still verify that the proposal is allowed at the control target and that the runtime response clearly identifies runtime target, control target, and evidence views.

## Relationship to experiments

Experiments are one possible analysis and governance strategy, not the entire product.

Decision intelligence may recommend an experiment when:

- multiple candidate values are plausible,
- evidence is insufficient to pick a winner,
- risk is acceptable,
- the action space is bounded,
- the outcome metrics are defined,
- the control target can tolerate exposure splitting.

For simple cases, the system may instead use qualitative analysis or deterministic rules. For mature cases, it may use bandits or models. The key is that the system chooses an appropriate reasoning strategy for the decision maturity and evidence quality.

## First hero scenario

For `tetris.dropInterval`, the initial decision intelligence loop can stay simple while still proving real-time adaptation:

```text
observe:
  hard-drop rate, placement time, early game-over rate, board pressure, recovery failures

interpret:
  new players are failing too quickly, but a single slower default may make stable sessions too easy

choose analysis mode:
  qualitative reasoning plus metric-threshold strategy

generate candidates:
  fixed 800ms
  fixed 900ms
  adaptive strategy from 600ms to 1100ms

evaluate:
  adaptive strategy can slow down under pressure and speed up after recovery,
  but must respect max-delta, cooldown, min/max range, and fallback

produce proposal:
  propose bounded adaptive strategy for cohort:new_players with expectedOutcome 0.72 and sufficient evidenceQuality

govern:
  approve if policy allows the range, step, cooldown, confidence report, and control target

online runtime:
  apply approved strategy to live session context and return immediate dropInterval

monitor:
  compare early game-over rate, recovery success, engagement, and stability after exposure
```

This proves the model without requiring a complex agent platform on day one.

## Design principles

1. **Proposal before decision**: AI produces candidates; governance produces runtime authority.
2. **Reasoning is scoped**: proposals must name where they apply and why that scope is appropriate.
3. **Evidence quality matters**: weak evidence should lead to hold, experiment, broader-scope fallback, or static fallback.
4. **Analysis mode is explicit**: qualitative reasoning, experiments, bandits, models, and LLM synthesis should be distinguishable.
5. **Runtime remains bounded**: online requests should not depend on unbounded agent loops.
6. **Policy remains authoritative**: the intelligence layer cannot override safety constraints.
7. **Outcome closes the loop**: decisions must feed future evidence, not end at response generation.
