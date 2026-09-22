# Decision Intelligence

## Purpose

Decision intelligence is the optional analysis and proposal-generation
capability used by the proposal-managed authority workflow. It operates over a
decision definition and decision evidence.

It answers:

> Given the declared objective, available evidence, uncertainty, current governed state, and known policy constraints, what bounded behavior should Flaggo recommend next?

Decision intelligence does not approve its own recommendation and does not serve application requests. It produces a `DecisionProposal` for a [decision lifecycle](DECISION_LIFECYCLES.md) to validate and govern. Approved state is applied per request by [runtime decision execution](RUNTIME_DECISION_EXECUTION.md).

```text
DecisionDefinition
  + DecisionEvidence
  + objectives
  + current GovernedDecisionState
  -> Decision Intelligence
  -> DecisionProposal

DecisionProposal
  -> Decision Lifecycle
  -> GovernedDecisionState

GovernedDecisionState
  -> Runtime Decision Execution
  -> RuntimeDecisionResult
```

This separation keeps Flaggo AI-native without making intelligence a
prerequisite for every usable decision or allowing reasoning to become policy
authority or request-time infrastructure.

## Relationship to authority workflows

Phase 3 intentionally does not invoke decision intelligence:

```text
definition bundle + initial authority candidate
  -> authenticated bundle approval
  -> governed state
```

Phase 4 introduces the proposal-managed path:

```text
definition + evidence + current state
  -> decision intelligence or another authorized producer
  -> DecisionProposal
  -> governance
  -> governed replacement state
```

Both paths use the same runtime state and execution boundary.

The remaining sections describe conceptual Phase 4 responsibilities and
deliverables. They do not define current proposal, experiment, rollout,
override, rollback, or fallback-only wire/state contracts.

## Core responsibility

Decision intelligence behaves like an embedded data scientist and operator assistant. It can:

1. Inspect declared objectives and available evidence.
2. Determine whether the evidence is sufficient and relevant.
3. Select an appropriate analysis mode.
4. Generate bounded candidate values or strategies.
5. Compare expected impact, uncertainty, and risk.
6. Recommend a value, strategy, experiment, rollout, hold, rollback, or fallback.
7. Explain the evidence and tradeoffs behind the recommendation.
8. Revisit recommendations as attributed outcomes arrive.

It does not:

- grant approval;
- mutate active authority directly;
- allocate experiment variants for runtime requests;
- route rollout traffic;
- bypass definition or environment policy;
- invent values outside the declared action space.

## Inputs

Decision intelligence consumes resolved, typed inputs rather than raw unbounded prompts:

- **Decision definition**: versioned semantic contract, objectives, action space, permitted workflows, and safety constraints.
- **Learning target**: target selected for analysis.
- **Candidate control target**: boundary where proposed authority would apply.
- **Evidence views**: historical observations and derived evidence sliced by target, time, and filters.
- **Decision and exposure records**: prior returned values, confirmed application, and attribution metadata.
- **Outcome evidence**: declared success metrics and guardrails.
- **Current governed state**: active authority and activation lineage, plus
  previous-safe-state, experiment, rollout, cooldown, or override data only
  when those lifecycle contracts exist.
- **Uncertainty**: evidence quality, sample size, freshness, variance, missingness, and model uncertainty.
- **Policy context**: constraints intelligence should consider before producing a proposal.

Runtime context may appear in historical decision records or exposure-captured inference inputs. Decision intelligence does not own the live request path.

## Output: DecisionProposal

The output is a proposal, not runtime authority.

A proposal may recommend:

- retaining or changing a fixed value;
- activating or revising a bounded strategy;
- starting, adjusting, or concluding an experiment;
- beginning, advancing, pausing, or rolling back a rollout;
- holding because evidence is insufficient;
- transitioning to a previous known-safe state;
- activating fallback-only state.

Every proposal should identify:

- exact decision definition identity;
- proposal type;
- learning and candidate control targets;
- candidate value, strategy, experiment, or rollout;
- evidence references and quality;
- expected impact and guardrail impact;
- model uncertainty when applicable;
- alternatives considered;
- rationale and known risks;
- requested approval mode;
- compatibility and supersession intent.

The proposal is passed to the lifecycle layer:

```text
DecisionProposal
  -> schema and compatibility validation
  -> effective policy evaluation
  -> governance disposition
  -> GovernedDecisionState, pending approval, or no authority change
```

## Async analysis path

Decision intelligence normally runs outside the application request path:

```text
telemetry change, schedule, operator request, definition activation,
experiment review, rollout review, or detected drift
  -> create intelligence work item
  -> resolve definition, learning target, evidence, and current state
  -> observe and interpret
  -> choose analysis mode
  -> generate and evaluate candidates
  -> produce DecisionProposal
```

The async path can tolerate longer-running reasoning, statistical analysis, tool use, and explanation generation. For adaptive runtime behavior, its most useful output is often a bounded strategy proposal rather than one fixed value.

## Analysis loop

```text
observe
  -> interpret
  -> choose analysis mode
  -> generate candidates
  -> evaluate candidates
  -> produce proposal
  -> monitor attributed outcomes
  -> improve future reasoning
```

### Observe

Collect evidence relevant to the definition and learning target:

- current and previous governed state;
- recent outcome telemetry;
- confirmed exposures;
- evidence inherited from broader targets;
- prior proposal and lifecycle history;
- active experiments and rollouts;
- policy-relevant state.

### Interpret

Translate evidence into decision-relevant findings, for example:

- a target is failing an objective;
- one guardrail improved while another regressed;
- evidence is fresh but the sample is too small;
- cohort evidence is reliable while user-level evidence is sparse;
- a prior state change improved the objective but increased risk.

### Choose analysis mode

| Mode | When useful |
| --- | --- |
| Qualitative reasoning | Evidence is sparse and an explainable hold or conservative recommendation is appropriate. |
| Heuristic analysis | Domain rules are simple and stable. |
| Experiment analysis | Controlled variants are active or uncertainty warrants proposing an experiment. |
| Bandit optimization | Approved exploration and exploitation among known candidates is appropriate. |
| Statistical model | Historical evidence can estimate candidate outcomes. |
| LLM-assisted synthesis | Goals, evidence, and tradeoffs require bounded explanation or synthesis. |
| Hybrid | Multiple methods contribute to one proposal. |

AI-native does not mean every proposal requires an LLM. The method must be explicit, bounded, and appropriate for the evidence.

### Generate candidates

Candidates must remain inside the definition-owned action space and workflow permissions.

For `tetris.dropInterval`, candidates might include:

- retain `800ms`;
- change to `850ms`;
- propose a strategy bounded between `600ms` and `1100ms`;
- propose an experiment comparing `800ms` and `900ms`;
- hold because evidence is insufficient;
- return to the previous known-safe state.

### Evaluate candidates

Evaluation should consider:

- expected objective impact;
- guardrail impact;
- evidence quality and freshness;
- model uncertainty;
- sample size and statistical power;
- target fit;
- blast radius and reversibility;
- likely policy eligibility;
- experiment or rollout requirements.

### Produce proposal

Phase 4 will define the concrete proposal DTO under issue #25. This document
keeps only the required boundary: a proposal identifies the exact definition
and target, describes one bounded candidate plus its rationale and supporting
evidence claims, and carries no trusted activation, state, approval, or
strategy identity. Proposal kinds and governance dispositions remain separate
concepts.

## Relationship to policy

Policy is authoritative and independently evaluated by the lifecycle layer. Decision intelligence may use policy context to avoid obviously invalid proposals, but it cannot:

- widen the action space;
- lower evidence-quality requirements;
- exceed rollout or experiment limits;
- bypass approval;
- ignore operator pause or override state when a separately approved operator
  contract exists;
- grant itself authority at a target.

## Relationship to targets

The learning target and proposed control target may differ from a future runtime target:

```text
runtime evidence originated from session:game-456
user-level evidence is sparse
cohort:new_players evidence is sufficient

learningTarget = cohort:new_players
proposed controlTarget = cohort:new_players
```

Choosing a broader target is an analysis result, not an authority grant. The lifecycle layer must still validate target authority and conflicts.

## Relationship to experiments

Experimentation is one way to create comparative evidence. Decision intelligence may propose an experiment when:

- multiple candidates remain plausible;
- existing evidence cannot identify a safe winner;
- the definition explicitly permits experimentation;
- success metrics and guardrails are declared;
- the proposed assignment target and traffic are within bounds.

Decision intelligence may also analyze active experiment outcomes and recommend continuing, stopping, changing allocation, promoting a winner, or rolling back. It does not activate the experiment or assign variants.

## Feedback and learning

```text
RuntimeDecisionResult
  -> confirmed exposure
  -> attributed outcome telemetry
  -> updated evidence views
  -> future DecisionProposal
```

Attribution prevents unused decisions, delayed outcomes, and unrelated observations from silently influencing future reasoning.

## Design principles

1. **Proposal, not authority**: intelligence recommends; governance authorizes.
2. **Async by default**: unbounded reasoning does not belong in the request path.
3. **Evidence before confidence**: weak evidence leads to hold, experiment, or conservative proposals.
4. **Analysis mode is explicit**: heuristics, experiments, models, bandits, and AI synthesis remain distinguishable.
5. **Reasoning is scoped**: every proposal names its learning and control targets.
6. **Contracts remain binding**: intelligence cannot invent actions or workflow permissions.
7. **Outcomes close the loop**: confirmed exposures and outcomes improve future proposals.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
- [Decision Lifecycles](DECISION_LIFECYCLES.md)
- [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md)
