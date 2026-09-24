# Reasoning Engine Design

## Purpose

The reasoning engine hosts two distinct seams: bounded runtime strategy
execution and optional async decision-intelligence proposal generation. They
remain separate architectural responsibilities and interfaces.

The current executor consumes existing governed state. #40 connects manifest
initial authority to the activation core; #41 re-baselines the integrated
runtime. Phase 4 may add a scripted or fixture-based proposal
source; it is not required to establish the Tetris authority.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Future proposal boundary: [Decision authority](../../architecture/AUTHORITY.md#proposal-managed-authority-phase-4).
Runtime execution model: [Runtime execution](../../architecture/RUNTIME_EXECUTION.md).

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
`IntelligenceLifecycleDefinitionSnapshot`, which exposes intent, declared
input/evidence semantics, result constraints, and safety envelope without
granting runtime authority.

## Online strategy executor port

```ts
interface INumericRuleExecutor {
  execute(input: NumericRuleExecutionRequest): Promise<NumericRuleExecutionResult>;
}

type NumericRuleExecutionRequest = {
  definition: RuntimeDefinitionProjection;
  rule: NumericRuleStrategy;
  inputs: Record<string, number | boolean | string>;
};

type NumericRuleExecutionResult = {
  candidate: number | null;
  reason: string;
  failureReason?: string;
};
```

`EvidenceSnapshot` is deliberately absent from this Phase 3 port. The executor
consumes only the resolved definition, the governed numeric rule, and a typed
primitive map. `DecisionInputResolver` validates request-owned operands and
resolves evidence-owned operands from one immutable generation under a
verified scope before execution. The executor cannot query telemetry, change
an input's ownership, or read a separately changing evidence snapshot.

Orchestration owns strategy identity and returns `confidence: null` for
deterministic rules; authored rationale and authenticated approval are
provenance, not learned confidence. Neither field is an executor dependency.

The Decision API resolves `active-value` authority directly. It calls
`INumericRuleExecutor` only after finding coherent `numeric-rule` authority and
passes the materialized rule rather than the whole state union. No compatible
active state at any permitted target is handled before strategy execution and
may resolve to the registered server fallback. Policy evaluation and fallback
selection occur after candidate production. Corrupt or incoherent state,
invalid strategy input, and non-ready registration are errors, not executor
fallback decisions.

## Numeric rule strategy behavior

```text
read each declared rule input from validated inputs by inputKey
  -> normalize each value to [0, 1] using declared minimum/maximum
  -> multiply by its declared weight
  -> divide the weighted sum by total weight
  -> compare the score with the declared threshold
  -> return valueAtOrAbove or valueBelow to policy
```

The division by finite positive total weight is mandatory; weights are not
required to sum to `1`. The threshold is compared with the normalized weighted
average, never the unnormalized sum.

Rules:

- Weighted inputs must be declared numeric operands, either request-owned or
  evidence-owned, with finite ranges and positive total weight.
- Missing, nonnumeric, or nonfinite required inputs make the
  strategy result invalid; they do not silently become zero.
- Numeric rule operands come only from `StrategyExecutionRequest.inputs`.
- Normalized input values are clamped to `[0, 1]`.
- Strategy execution never applies fallback directly; it returns a candidate
  or a typed execution error.
- Policy remains responsible for output bounds, step, applicable max delta, and
  fallback.

## Runtime condition evaluation

The numeric rule consumes only resolved inputs declared in manifest `inputs`.
Missing, stale, future, ambiguous, or invalid required input evidence produces
an explicit 503 before execution; it never becomes zero or an SDK fallback.
Request-only decisions without evidence-dependent policy read neither evidence
port. Policy-quality evidence remains separate and does not manufacture
confidence for a deterministic rule. Arbitrary queries, rolling aggregation,
and stateful condition languages are not part of this contract.

## Phase 4 async proposal source

Phase 4 may begin with a scripted or fixture-backed proposal source. Its
producer port and proposal DTO are intentionally deferred until the Phase 4
contract is designed. Any future producer remains outside the online executor
and cannot write active state directly.

## Tetris MVP strategy

Existing Tetris governed rule (trusted local fixture until #40/#41):

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

These Tetris weights total `1`, so the displayed numerator already equals the
normalized weighted average. The executor still applies the canonical total
weight denominator.

This proves real-time contextual adaptation without claiming learned evidence
or building proposal-generation infrastructure.

## MVP non-goals

- LLM calls in the online runtime path.
- Model training.
- Bandit allocation.
- Experiment analysis.
- Multi-step autonomous agent workflows.

Those should be future strategy or intelligence implementations behind the same ports.
