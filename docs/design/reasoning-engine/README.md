# Reasoning Engine Design

## Purpose

The reasoning engine hosts two distinct seams: bounded runtime strategy
execution and optional async decision-intelligence proposal generation. They
remain separate architectural responsibilities and interfaces.

Phase 3 builds and validates the online strategy executor against
bundle-approved state. Phase 4 may add a scripted or fixture-based proposal
source; it is not required to establish the Tetris authority.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Decision intelligence model: [Decision Intelligence](../../DECISION_INTELLIGENCE.md).
Runtime execution model: [Runtime Decision Execution](../../RUNTIME_DECISION_EXECUTION.md).

## MVP split

| Area | MVP behavior |
| --- | --- |
| Phase 3 online strategy execution | Deterministically execute an active bundle-approved `numeric-rule` in the Decision API request path. |
| Phase 3 authority input | Consume state activated from an authenticated bundle approval; do not generate authority. |
| Phase 4 async intelligence | Produce independent proposals that use the same activation boundary. |

The online executor must be fast and bounded. A later proposal source may
become agentic without changing runtime execution.

## Definition projection ownership

Runtime reasoning receives a typed `RuntimeDefinitionProjection` through the
registry-owned runtime read port. It does not parse definition bundles or
registry persistence. Target resolution uses the projection's inference target
followed by its explicit fallback order; target hierarchy is an authorization
boundary, not an implied precedence list. Missing required context,
inconsistent target bindings, and runtime or governed-state targets outside
that boundary fail closed before strategy execution.

Async proposal generation receives the separate
`IntelligenceLifecycleDefinitionSnapshot`, which exposes objectives, signal
roles, workflow permissions, action space, and safety envelope without
granting runtime authority.

## Online strategy executor port

```ts
interface IStrategyExecutor {
  execute(input: StrategyExecutionRequest): Promise<StrategyExecutionResult>;
}

type StrategyExecutionRequest = {
  definition: DecisionDefinition;
  state: DecisionState;
  evidence?: EvidenceSnapshot;
  runtimeContext: RuntimeContext;
  now: string;
};

type StrategyExecutionResult = {
  value: DecisionValue;
  decisionMode: "active-value" | "strategy" | "experiment" | "fallback";
  strategyId?: string;
  confidence: ConfidenceReport | null;
  reason: string;
};
```

Bundle-authored strategies return `confidence: null`; authored rationale and
authenticated approval are provenance, not learned confidence.

`IStrategyExecutor` should only be called after the Decision API has found an active `DecisionState` with an active value or active strategy. Missing state should be handled before strategy execution and should resolve to contract fallback or policy fallback.

## Numeric rule strategy behavior

```text
read each declared inference input from runtimeContext
  -> normalize each value to [0, 1] using declared minimum/maximum
  -> multiply by its declared weight
  -> divide the weighted sum by total weight
  -> compare the score with the declared threshold
  -> return valueAtOrAbove or valueBelow to policy
```

Rules:

- Weighted inputs must be declared inference inputs with finite ranges and
  positive total weight.
- Missing, duplicate, nonnumeric, or nonfinite required inputs make the
  strategy result invalid; they do not silently become zero.
- Normalized input values are clamped to `[0, 1]`.
- Strategy execution should not apply fallback directly unless no candidate can be produced.
- Policy remains responsible for output bounds, step, applicable max delta, and
  fallback.

## Runtime condition evaluation

The Phase 3 numeric rule consumes only the live inputs declared by
`inference.inputs`. Evidence-backed or stateful condition languages are not
part of this rule contract.

## Phase 4 async proposal source

```ts
interface IDecisionIntelligence {
  propose(input: IntelligenceRequest): Promise<DecisionProposal>;
}
```

Phase 4 can implement `IDecisionIntelligence` as:

- a fixture loader,
- a script that creates a replacement proposal,
- a simple heuristic that returns a fixed `StrategyProposal`.

The important point is that proposal output uses the same `DecisionProposal` and `DecisionStrategy` contracts future AI agents will use.

## Tetris MVP strategy

Initial bundle-approved strategy:

```text
score =
  normalize(boardPressure, 0..1) * 0.45
  + normalize(recentPlacementTimeMs, 0..2000) * 0.25
  + normalize(recoveryFailures, 0..5) * 0.20
  + normalize(currentLevel, 0..20) * 0.10

if score >= 0.55:
  return 850
else:
  return 750
```

This proves real-time contextual adaptation without claiming learned evidence
or building proposal-generation infrastructure.

## MVP non-goals

- LLM calls in the online runtime path.
- Model training.
- Bandit allocation.
- Experiment analysis.
- Multi-step autonomous agent workflows.

Those should be future strategy or intelligence implementations behind the same ports.
