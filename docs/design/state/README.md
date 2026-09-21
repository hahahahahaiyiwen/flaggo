# State Design

## Purpose

The state component owns live runtime authority for a decision definition and target. Contracts describe what a decision means; state describes what is currently active.

For the MVP, state is intentionally small and local-first. It should support the Tetris `dropInterval` adaptive strategy without requiring cloud storage.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

The Phase 3 activation core stores:

- state ID and monotonic generation,
- application, environment, decision key, and control target address,
- definition ID, revision, and contract digest,
- active value or numeric rule,
- predecessor state ID,
- proposal, activation, and approval references,
- activation timestamp and lifecycle status.

Runtime lookup remains read-only. Activation uses durable local persistence for
the MVP; future adapters may use SQLite, PostgreSQL, Redis, or a cloud store
behind the same boundary.

## Core ports

```ts
interface IStateStore {
  getActiveState(input: StateRequest): Promise<DecisionState | null>;
}

type StateRequest = {
  definition: DecisionDefinitionRef;
  controlTarget?: DecisionTargetRef;
  runtimeTarget?: DecisionTargetRef;
};

interface IStateActivationStore {
  getBaseline(input: StateAddress): Promise<DecisionState | null>;
  activate(input: ActivationRequest): Promise<DecisionState>;
}

type StateAddress = {
  appId: string;
  environment: string;
  decisionKey: string;
  controlTarget: DecisionTargetRef;
};

type ActivationRequest = {
  activationId: string;
  proposalId: string;
  approvalReference: string;
  definition: DecisionDefinitionRef;
  address: StateAddress;
  expectedBaseline: {
    stateId?: string;
    generation: number;
  };
  candidate: {
    kind: "numeric-rule";
    rule: NumericRuleDeclaration;
    rationale: string;
  };
};
```

Runtime lookup receives an ordered set of exact targets from reasoning after
the registry-owned runtime projection has authorized the target kinds.
`IStateStore` owns storage and exact tuple matching; it does not derive
hierarchy, add implicit user/cohort/global fallback, or parse registry
definitions. Reasoning rejects any returned control target that is not one of
the requested permitted targets.

The activation port, not a proposal producer or runtime caller, constructs the
durable state. It retains state identity and generation, expected-baseline
compare-and-swap, idempotent replay, predecessor and approval references,
validation conflicts, and atomic publication. Broader completion, expiry,
rollback, evidence, confidence, and generic proposal-source surfaces are
deferred until a concrete lifecycle requires them.

## Runtime behavior

```text
Decision API
  -> resolves runtime target and control target
  -> loads governed control state for definition + control target, if present
  -> executes active strategy or active value
  -> applies deterministic runtime policy
```

Precedence:

1. Retired contract forces fallback.
2. Active numeric rule produces an adaptive runtime value.
3. Active value returns a fixed governed value.
4. Missing or invalid state returns the contract fallback.

Pause, override, cooldown, previous-result delta, hysteresis, and other
temporal behavior require explicit follow-up contracts rather than implicit
state-store mutation.

## Tetris MVP state

Example active state:

```json
{
  "decisionKey": "tetris.dropInterval",
  "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
  "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "contractDigest": "sha256:...",
  "mode": "strategy",
  "strategyId": "strategy-tetris-new-players-v1",
  "numericRule": {
    "threshold": 0.55,
    "valueAtOrAbove": 850,
    "valueBelow": 750,
    "weightedInputs": [
      {
        "signalKey": "tetris.boardPressure",
        "minimum": 0,
        "maximum": 1,
        "weight": 0.45
      },
      {
        "signalKey": "tetris.recentPlacementTimeMs",
        "minimum": 0,
        "maximum": 2000,
        "weight": 0.25
      },
      {
        "signalKey": "tetris.recoveryFailures",
        "minimum": 0,
        "maximum": 5,
        "weight": 0.2
      },
      {
        "signalKey": "tetris.currentLevel",
        "minimum": 0,
        "maximum": 20,
        "weight": 0.1
      }
    ]
  },
  "stateId": "state_01...",
  "proposalId": "proposal_01...",
  "activationId": "activation_01...",
  "generation": 1,
  "predecessorStateId": null,
  "approvalReference": "approval_01...",
  "lifecycle": "active"
}
```

## State isolation across contracts

Decision state is not the same thing as telemetry. State represents live
authority: active strategy or active value plus its activation lineage.
Governed control state must be isolated by decision definition plus control
target.

Rules:

- A new semantic decision definition gets a new state namespace by default.
- Old builds can continue using their known definition ID and state while new builds use a new definition ID or semantic revision.
- Raw telemetry and compatible evidence views may be reused to avoid cold start, but active strategy/state should not be copied automatically.
- If a team wants to seed a new contract from old state, that should be an explicit migration with audit records and policy checks.

## MVP non-goals

- Distributed locking.
- Multi-region consistency.
- Complex rollout state.
- Completion, expiry, rollback, pause, and override workflows.
- Temporal stabilization semantics.
- Long-term state history beyond audit.

Those can be added later behind `IStateStore` and audit records.
