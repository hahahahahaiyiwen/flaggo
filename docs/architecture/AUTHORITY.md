# Decision authority

## Purpose

Decision authority is the control-plane boundary that turns a bounded candidate
into durable behavior that runtime may consume.

It answers:

> How does declared or proposed behavior become active, remain ordered under
> concurrency, and become superseded?

Authority does not select a value for an individual request. Runtime execution
does not approve or mutate authority.

## Boundary

| Capability | Owns | Does not own |
| --- | --- | --- |
| Candidate or proposal producer | A bounded value or strategy plus rationale. | Approval or active state. |
| Authority workflow | Authentication, exact-snapshot approval, activation, concurrency, replay, supersession, readiness, and lifecycle audit. | Per-request value selection. |
| Runtime execution | Applying compatible approved state to one request. | Candidate generation, approval, or state mutation. |

Authority always comes from authenticated governance and activation, never from
the candidate source.

## Bundle-approved authority

Phase 3 uses a definition-bundle candidate and does not invoke decision
intelligence:

```text
bundle apply
  -> validate definition and initial authority candidate
  -> create exact-snapshot approval request
  -> authenticated approval
  -> durably allocate server-derived identities
  -> capture expected stable-head baseline
  -> expected-baseline compare-and-swap
  -> publish immutable GovernedDecisionState
  -> return ready registration receipt
```

The bundle cannot provide trusted approval, proposal, activation, state, or
numeric-rule strategy identities. For each definition, the server derives
proposal and activation identities from the application, environment, approval
request, decision key, contract digest, and canonical candidate. Numeric-rule
activation also derives its strategy identity; active-value activation has no
strategy identity.

The initial authority candidate participates in semantic identity. Changing its
control target, kind, value or rule, or rationale requires a new definition
revision and approval.

### Replay and interrupted activation

Exact retry reuses the stored approval, identities, expected baseline, state
ID, generation, and numeric-rule strategy ID. It never substitutes the
authority head visible at retry time.

An interrupted or outcome-unknown activation resumes with the same identities.
A stale baseline, changed candidate, or permanent activation conflict remains
non-ready and cannot overwrite newer authority. Reapplying unchanged accepted
content may create one linked authority-reauthorization approval for only the
failed authorities; concurrent exact reapplies must converge on that successor.

### Registration readiness

Approval and activation are separate events. A bundle-approved definition with
required initial authority is not ready until activation succeeds. The ready
receipt exposes the accepted runtime identity and the derived proposal,
activation, state, generation, target, and authority-kind references.

## Proposal-managed authority (Phase 4)

Phase 4 may add independently produced candidates:

```text
definition + evidence + current state when present
  -> authorized producer
  -> bounded DecisionProposal
  -> governance
  -> initial or replacement activation
  -> GovernedDecisionState
```

The producer may be decision intelligence, an operator, or other authorized
automation. It cannot write runtime authority directly or supply trusted
activation, state, approval, or strategy identities.

The first proposal-managed activation uses the shared no-state baseline:
generation `0`, no `stateId`, and no predecessor on the resulting state. Later
activations compare against the captured current head and create replacement
state.

This section is an extension boundary, not a proposal DTO, analysis workflow,
governance enum, experiment model, rollout model, or operator contract. GitHub
issue #25 owns those details.

## Shared activation model

The stable authority address is:

```text
application + environment + decision key + control target
```

It identifies one ordered authority head across semantic revisions.

| Concept | Invariant |
| --- | --- |
| Expected baseline | Approval preparation captures and stores the exact head that activation expects. |
| Compare-and-swap | Activation succeeds only when the stable head still matches that captured baseline. |
| Immutable state | Successful activation creates one exact-definition state record. |
| Predecessor | Replacement state names the state it superseded; first authority has no predecessor. |
| Generation | Successful publication advances the stable head monotonically. |
| Replay | Exact retry returns the original publication rather than allocating another state. |
| Atomic publication | Runtime never observes a partially published state/head pair. |

A well-formed stale activation conflicts. It does not create another revision
namespace or bypass the stable head.

## Current authority kinds

Current governed state contains exactly one authority kind:

| Kind | State payload | Ready receipt |
| --- | --- | --- |
| `active-value` | One approved contract-valid value. | Omits `strategyId`. |
| `numeric-rule` | One approved deterministic weighted threshold rule. | Requires the derived `strategyId`. |

Both kinds share validation, captured-baseline activation, replay, stale-head
conflict, predecessor lineage, and atomic publication.

Governed fallback is a runtime outcome, not a persisted authority kind.
Experiment, rollout, fallback-only, and override authority remain future
concepts.

## Current lifecycle

```text
bundle candidate:
  declared -> approval-pending -> authorized -> activated

GovernedDecisionState:
  active -> superseded
```

Expiry, completion, rollback, pause, override, and broader public transition
history require separately approved contracts. Rollback, when designed, should
activate replacement or previous known-safe authority rather than become an
implicit payload kind.

## Authorization, policy, and audit

Bundle approval authorizes the exact immutable snapshot. The approval actor and
comment are persisted before state activation. The server validates the
candidate against the exact definition, target hierarchy, output contract,
fallback, and applicable policy.

Effective policy is the intersection of definition constraints, environment
policy, and any separately approved operator controls. A less-trusted layer may
narrow behavior but cannot widen it.

Lifecycle audit must reconstruct:

- the exact definition identity and candidate;
- authenticated approval and timestamp;
- server-derived proposal, activation, state, and strategy identities;
- expected baseline and stable authority address;
- previous and replacement state IDs;
- validation, policy, activation, retry, and conflict outcomes.

## Runtime handoff

Runtime consumes a read-only projection of compatible active state:

```text
DecisionDefinition
  + runtime target and live inputs
  + compatible GovernedDecisionState
  + runtime policy
  -> RuntimeDecisionResult
```

The authority source is intentionally irrelevant to runtime. Bundle-approved
and future proposal-managed paths must produce the same current state shape or
introduce a separately approved extension.

## Design invariants

1. Candidates never self-approve.
2. The server derives trusted lifecycle and state identities.
3. Approval and activation are distinct, durable events.
4. One stable head orders authority across semantic revisions.
5. Activation uses the baseline captured during approval.
6. Exact replay returns the original publication.
7. Current state contains only `active-value` or `numeric-rule` authority.
8. Runtime reads authority but cannot create it.
9. Proposal-managed detail remains deferred to #25.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Evidence](EVIDENCE.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Tetris scenario](../scenarios/TETRIS.md)
- [State component](../design/state/README.md)
- [Contract and registry component](../design/contract-registry/README.md)
