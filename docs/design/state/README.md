# State Design

## Purpose

The state component owns live runtime authority for a stable decision and
control-target address. Contracts describe what a semantic revision means;
each immutable state record identifies which exact revision is currently
active at that address.

For the MVP, state is intentionally small and local-first. It should support the Tetris `dropInterval` adaptive strategy without requiring cloud storage.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

The Phase 3 activation core stores:

- state ID and monotonic generation,
- stable application, environment, decision key, and control target address,
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
  contractDigest: string;
  resolutionTargets: DecisionTargetRef[];
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
  contractDigest: string;
  address: StateAddress;
  expectedBaseline: ExpectedAuthorityBaseline;
  candidate: {
    kind: "numeric-rule";
    rule: NumericRuleDeclaration;
    rationale: string;
  };
};
```

Runtime lookup receives an ordered set of exact targets from reasoning after
the registry-owned runtime projection has authorized the target kinds. The
resolver starts with the exact primary `inference.target`, then appends only
the exact target kinds named by `inference.fallbackOrder`. For Tetris that is
`session -> cohort -> global`; `user` is not inserted implicitly.

`IStateStore` owns storage and exact tuple matching; it does not derive
hierarchy, add implicit fallback targets, or parse registry definitions. It
reads the stable authority head for each requested target in order and returns
the first state whose definition identity exactly matches the request. A
well-formed head for another semantic revision is incompatible and may be
skipped. `getActiveState` never returns a `superseded` projection. A
superseded return, malformed/torn state, or internally incoherent payload is a
readiness failure, not an absent-state result.

The activation port, not a proposal producer or runtime caller, constructs the
durable state and derives the opaque strategy identity from the activation ID
and canonical ID-free strategy declaration. The stable `StateAddress`
intentionally excludes definition revision: every semantic revision
compare-and-swaps the same authority head, so a stale activation cannot bypass
newer authority by writing another namespace. The port retains state identity
and generation, idempotent replay, predecessor and approval references,
validation conflicts, and atomic publication. Broader completion, expiry,
rollback, evidence, confidence, and generic proposal-source surfaces are
deferred until a concrete lifecycle requires them.

`getBaseline` is used while preparing an approval transition, not afresh on
each activation attempt. The control plane atomically persists the observed
`ExpectedAuthorityBaseline` with the deterministic activation ID before
reporting approval. `activate` and every exact retry receive that stored value.
If another activation advances the stable head afterward, compare-and-swap
fails stale rather than adopting the newer head as the expected baseline.
After that permanent failure, an explicitly approved linked
authority-reauthorization receives a new deterministic activation identity and
captures the then-current stable head as its own baseline. It reuses the
accepted definition revision and does not replace successful sibling
activations from the original bundle.

## Runtime behavior

```text
Decision API
  -> validates registration readiness and exact definition identity
  -> resolves ordered exact targets from inference target + fallbackOrder
  -> loads the first compatible state from stable authority heads
  -> executes active strategy or active value
  -> applies deterministic runtime policy
```

Precedence:

Contract identity and lifecycle eligibility are validated before state
resolution. A retired identity returns `409 retired-definition`; it never
reaches state precedence or fallback.

1. Active numeric rule produces an adaptive runtime value.
2. Active value returns a fixed governed value.
3. No compatible active state at any permitted target returns the audited
   contract fallback.

Pending activation, a failed readiness check, corrupt persistence, or a state
that violates `DecisionState` invariants is a contract/readiness error. It must
not be recast as `missing_state` fallback.

`DecisionState` is an exactly-one discriminated union. `authorityKind:
"active-value"` requires only `activeValue`; `authorityKind: "numeric-rule"`
requires only `activeStrategy`. Both-present, neither-present, fixed-value
strategy, or discriminator/payload mismatch fails readiness before lookup.

Pause, override, cooldown, previous-result delta, hysteresis, and other
temporal behavior require explicit follow-up contracts rather than implicit
state-store mutation.

## Tetris MVP state

Example active state:

```json
{
  "stateId": "state_01...",
  "proposalId": "proposal_01...",
  "activationId": "activation_01...",
  "definition": {
    "appId": "tetris-demo",
    "environment": "dev",
    "key": "tetris.dropInterval",
    "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
    "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
  },
  "contractDigest": "sha256:...",
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "generation": 1,
  "approvalReference": "approval_01...",
  "activatedAt": "2026-07-25T12:00:00Z",
  "lifecycle": "active",
  "authorityKind": "numeric-rule",
  "activeStrategy": {
    "kind": "numeric-rule",
    "id": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
    "threshold": 0.55,
    "valueAtOrAbove": 850,
    "valueBelow": 750,
    "weightedInputs": [
      {
        "signal": { "key": "tetris.boardPressure" },
        "minimum": 0,
        "maximum": 1,
        "weight": 0.45
      },
      {
        "signal": { "key": "tetris.recentPlacementTimeMs" },
        "minimum": 0,
        "maximum": 2000,
        "weight": 0.25
      },
      {
        "signal": { "key": "tetris.recoveryFailures" },
        "minimum": 0,
        "maximum": 5,
        "weight": 0.2
      },
      {
        "signal": { "key": "tetris.currentLevel" },
        "minimum": 0,
        "maximum": 20,
        "weight": 0.1
      }
    ]
  }
}
```

## Stable authority head and contract isolation

Decision state is not the same thing as telemetry. State represents live
authority: active strategy or active value plus its activation lineage.
The mutable authority head is shared by semantic revisions only as the
serialization point for one decision key and control target. The state record
referenced by that head remains bound to one exact definition identity.

Rules:

- `StateAddress = { appId, environment, decisionKey, controlTarget }` is stable
  across semantic revisions of that decision.
-   Activating a new revision compare-and-swaps the current head and increments its
  generation. The read/history projection then reports the retained predecessor
  as superseded without changing its authority payload or definition binding.
- A request for an older registered revision never consumes the newer
  strategy. If no permitted authority head contains an exact compatible state,
  it receives the audited server fallback.
- Raw telemetry and compatible evidence views may be reused to avoid cold
  start, but an active strategy/state record is never copied or reinterpreted
  under another definition identity.

## MVP non-goals

- Distributed locking.
- Multi-region consistency.
- Complex rollout state.
- Completion, expiry, rollback, pause, and override workflows.
- Temporal stabilization semantics.
- Long-term state history beyond audit.

Those can be added later behind `IStateStore` and audit records.
