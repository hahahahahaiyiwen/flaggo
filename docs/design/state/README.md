# State Design

## Purpose

The state component owns live runtime authority for a decision definition and target. Contracts describe what a decision means; state describes what is currently active.

For the MVP, state is intentionally small and local-first. It should support the Tetris `dropInterval` adaptive strategy without requiring cloud storage.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

The implemented state boundary stores:

- active value or active strategy,
- exact definition/control-target identity and immutable history,
- state/proposal/approval identities and monotonic generations,
- activation and last-change time,
- supersession, rollback, completion, and expiry status,
- lifecycle review/approval/audit records and immutable replay receipts.

In-memory and local committed-file adapters implement the same atomic
lifecycle boundary. Runtime is read-only; per-request cooldown/history and
general override state are not added by this slice.

## Separate read and lifecycle ports

```csharp
interface IStateStore {
  Task<GovernedDecisionState?> GetActiveAsync(
    string decisionKey, string definitionId, string revision,
    IReadOnlyList<DecisionTargetRef?> resolutionTargets,
    CancellationToken cancellationToken);
}

interface IGovernedStateLifecycleStore {
  // Baseline, history, and immutable receipt reads are also exposed.
  Task<LifecycleReviewReceipt> CommitReviewAsync(
    LifecycleReviewCommit commit, CancellationToken cancellationToken);
  Task<LifecycleActivationReceipt> CommitActivationAsync(
    LifecycleActivationCommit commit, CancellationToken cancellationToken);
  Task<LifecycleTransitionReceipt> CommitTransitionAsync(
    LifecycleTransitionCommit commit, CancellationToken cancellationToken);
}
```

`IProposalGovernance` is the producer-facing application boundary. It supplies
authenticated actor authority and evaluated policy to these trusted commits;
producers cannot write arbitrary state or bypass approval. Activation requires
a recorded automatic approval, fresh reviewed inputs, non-expiry, and the
expected baseline state ID/generation. Human-required reviews stay pending.

Version 3 persistence co-commits state history, lifecycle audit, approval, and
replay receipts through immutable artifacts and descriptor-last publication.
Runtime reconstructs the proof before exposing active state. Exact replay
returns the original receipt, while current state status is read separately.
Cancellation/timeout after publication can leave a committed outcome; retry
the same identity. An underlying writer retains its lease until completion.
Version 2 lifecycle persistence is removed, without migration. Version 1
standalone demo bootstrap remains read-only and cannot carry lifecycle proof.

Runtime lookup receives an ordered set of exact targets from reasoning after
the registry-owned runtime projection has authorized the target kinds.
`IStateStore` owns storage and exact tuple matching; it does not derive
hierarchy, add implicit user/cohort/global fallback, or parse registry
definitions. Reasoning rejects any returned control target that is not one of
the requested permitted targets.

## Runtime behavior

```text
Decision API
  -> resolves runtime target and control target
  -> loads governed control state for definition + control target, if present
  -> loads runtime target state for definition + runtime target, if needed
  -> checks override or pause
  -> executes active strategy or active value
  -> returns an audited value without mutating governed authority
```

Precedence:

1. Retired contract forces fallback.
2. Pause state forces fallback or existing safe value.
3. Operator override takes precedence over active strategy.
4. Active strategy produces adaptive runtime value.
5. Active value returns fixed governed value.
6. Missing state returns contract fallback.

## Tetris MVP state

Illustrative runtime projection of an activated state, not a standalone
persistence document:

```json
{
  "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
  "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
  "contractDigest": "sha256:6eadd7bd76b36ae06e89376d57107da83fdcabf07ff58c528ae97fddb7f08ee9",
  "value": 800,
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "mode": "strategy",
  "strategyId": "strategy-tetris-new-players-v1",
  "numericRule": {
    "inputSignalKey": "tetris.boardPressure",
    "threshold": 0.7,
    "valueAtOrAbove": 850,
    "valueBelow": 750
  },
  "stateId": "state-1",
  "proposalId": "proposal-1",
  "generation": 1,
  "predecessorStateId": null,
  "approvalReference": "approval-record-id",
  "activatedAt": "2026-09-20T20:00:00Z",
  "lastChangedAt": "2026-09-20T20:00:00Z",
  "lifecycleStatus": "active"
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
- Cross-store transactions that separate lifecycle audit from authority.

Future adapters must preserve the lifecycle co-commit invariant while exposing
only the read-only `IStateStore` projection to runtime.
