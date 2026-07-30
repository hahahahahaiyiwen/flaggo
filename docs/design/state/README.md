# State Design

## Purpose

The state component owns live runtime authority for a decision definition and target. Contracts describe what a decision means; state describes what is currently active.

For the MVP, state is intentionally small and local-first. It should support the Tetris `dropInterval` adaptive strategy without requiring cloud storage.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

State stores:

- active value or active strategy,
- previous value,
- last decision time,
- cooldown deadline,
- pause state,
- operator override,
- definition ID/revision tied to the active state.

MVP state should support in-memory storage first. A later persistent implementation can use SQLite, PostgreSQL, Redis, or a cloud store behind the same interface.

## Core port

```ts
interface IStateStore {
  getActiveState(input: StateRequest): Promise<DecisionState | null>;
  updateActiveState(input: StateUpdate): Promise<void>;
}

type StateRequest = {
  definition: DecisionDefinitionRef;
  controlTarget?: DecisionTargetRef;
  runtimeTarget?: DecisionTargetRef;
};

type StateUpdate = {
  definition: DecisionDefinitionRef;
  controlTarget?: DecisionTargetRef;
  runtimeTarget?: DecisionTargetRef;
  expectedContractVersion?: string;
  nextState: DecisionState;
  reason: string;
};
```

## Runtime behavior

```text
Decision API
  -> resolves runtime target and control target
  -> loads governed control state for definition + control target, if present
  -> loads runtime target state for definition + runtime target, if needed
  -> checks override or pause
  -> executes active strategy or active value
  -> updates lastDecisionAt/cooldown when needed
```

Precedence:

1. Retired contract forces fallback.
2. Pause state forces fallback or existing safe value.
3. Operator override takes precedence over active strategy.
4. Active strategy produces adaptive runtime value.
5. Active value returns fixed governed value.
6. Missing state returns contract fallback.

## Tetris MVP state

Example active state:

```json
{
  "decisionKey": "tetris.dropInterval",
  "definition": "tetris.dropInterval@2",
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "contractVersion": "1",
  "lifecycle": "active",
  "activeStrategy": {
    "kind": "numeric-rule",
    "id": "strategy-tetris-new-players-v1",
    "baseValue": 800,
    "min": 600,
    "max": 1100,
    "step": 50,
    "cooldownSeconds": 20,
    "rules": []
  },
  "previousValue": 800
}
```

## State isolation across contracts

Decision state is not the same thing as telemetry. State represents live authority: active strategy, active value, cooldown, pause, override, and rollback transition metadata. Governed control state must be isolated by decision definition plus control target. Runtime target state must be isolated by decision definition plus runtime target.

Rules:

- A new semantic decision definition gets a new state namespace by default.
- Old builds can continue using their known definition ID and state while new builds use a new definition ID or semantic revision.
- Raw telemetry and compatible evidence views may be reused to avoid cold start, but active strategy/state should not be copied automatically.
- If a team wants to seed a new contract from old state, that should be an explicit migration with audit records and policy checks.

## MVP non-goals

- Distributed locking.
- Multi-region consistency.
- Complex rollout state.
- Long-term state history beyond audit.

Those can be added later behind `IStateStore` and audit records.
