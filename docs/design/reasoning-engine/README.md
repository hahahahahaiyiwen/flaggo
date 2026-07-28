# Reasoning Engine Design

## Purpose

The reasoning engine is the seam for decision intelligence and bounded strategy execution.

For the MVP, do not build a full AI agent platform. Build the online strategy executor and keep async intelligence as a scripted or fixture-based proposal source that uses the same shared contracts future agents will use.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Decision intelligence model: [Decision Intelligence](../../DECISION_INTELLIGENCE.md).

## MVP split

| Area | MVP behavior |
| --- | --- |
| Online strategy execution | Deterministically execute active `numeric-rule` strategies in the Decision API request path. |
| Async intelligence | Produce or load scripted `StrategyProposal` objects for Tetris. |
| Governance | Review and activate proposals into `DecisionState`. |

The online executor must be fast and bounded. The async proposal source may later become agentic.

## Online strategy executor port

```ts
interface IStrategyExecutor {
  execute(input: StrategyExecutionRequest): Promise<StrategyExecutionResult>;
}

type StrategyExecutionRequest = {
  definition: DecisionDefinition;
  state: DecisionState;
  evidence: EvidenceSnapshot;
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

`IStrategyExecutor` should only be called after the Decision API has found an active `DecisionState` with an active value or active strategy. Missing state should be handled before strategy execution and should resolve to contract fallback or policy fallback.

## Numeric rule strategy behavior

```text
start with baseValue or previousValue
  -> evaluate rules against runtimeContext and evidence metrics
  -> apply matching adjustments
  -> clamp to strategy min/max
  -> align to step
  -> return candidate to policy
```

Rules:

- Multiple matching rules may be applied in order unless later policy chooses otherwise.
- Missing facts should make a condition false, not throw.
- Strategy execution should not apply fallback directly unless no candidate can be produced.
- Policy remains responsible for approval, cooldown, max delta, and fallback.

## Runtime condition evaluation

Conditions support:

- `all`,
- `any`,
- primitive fact lookup,
- comparison operators: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`.

Fact sources:

1. `runtimeContext`,
2. `evidence.metrics`,
3. selected state facts if explicitly exposed.

MVP should start with runtime context facts only unless evidence metrics are already available.

## Async proposal source

```ts
interface IDecisionIntelligence {
  propose(input: IntelligenceRequest): Promise<DecisionProposal>;
}
```

For MVP, `IDecisionIntelligence` can be implemented as:

- a fixture loader,
- a script that creates the Tetris strategy proposal,
- a simple heuristic that returns a fixed `StrategyProposal`.

The important point is that proposal output uses the same `DecisionProposal` and `DecisionStrategy` contracts future AI agents will use.

## Tetris MVP strategy

Initial strategy:

```text
baseValue = 800
range = 600..1100
step = 50
cooldown = 20s

if boardPressure >= 0.7 and recentPlacementTimeMs >= 1200:
  adjust +50

if boardPressure == low and recentPlacementTimeMs <= 700:
  adjust -50
```

This proves real-time adaptation without building complex modeling infrastructure.

## MVP non-goals

- LLM calls in the online runtime path.
- Model training.
- Bandit allocation.
- Experiment analysis.
- Multi-step autonomous agent workflows.

Those should be future strategy or intelligence implementations behind the same ports.
