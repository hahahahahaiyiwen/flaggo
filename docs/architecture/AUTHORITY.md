# Decision authority

## Purpose

Decision authority is the control-plane boundary that turns a bounded candidate
into immutable state that Decision Service may consume.

Contract Service owns approval and activation orchestration. State Store owns
atomic authority persistence.

## Boundary

| Boundary | Owns | Cannot do |
| --- | --- | --- |
| Candidate producer | Bounded value or rule plus rationale. | Approve or activate itself. |
| Contract Service | Authentication, exact approval, constraint validation, baseline capture, activation orchestration, and readiness. | Select one request-time value. |
| State Store | Stable heads, CAS, immutable state, replay, lineage, and atomic publication. | Approve candidates. |
| Decision Service | Read compatible active state and execute it. | Mutate authority. |

## Bundle-approved authority

The following is the accepted Phase 3 extension owned by #40. The current
manifest-first v2 contract publishes definitions but does not yet declare
initial authority or claim ready-after-activation receipts. Local examples
provision state through trusted fixtures. The activation core remains the
boundary this extension will use after #49's server alignment; it does not
invoke decision intelligence:
```text
definition + initial authority candidate
  -> Contract Service validation
  -> authenticated exact-snapshot approval
  -> durable server-derived identities
  -> captured stable-head baseline
  -> State Store compare-and-swap
  -> ready runtime binding
```

The bundle cannot provide trusted approval, proposal, activation, state, or
numeric-rule strategy identities.

Changing target, kind, value/rule, or rationale creates a semantic change that
requires a new definition revision and approval.

## Proposal-managed authority

Future Async Analysis Pipeline work may submit a bounded candidate:

```text
contracts + evidence + current state
  -> Async Analysis Pipeline candidate
  -> Contract Service governance
  -> State Store activation
```

The pipeline may be agentic or long-running. It never writes the active head
directly. Issue #25 owns the first proposal-managed contract.

## Stable activation model

The stable authority address is:

```text
application + environment + decision key + control target
```

| Invariant | Requirement |
| --- | --- |
| Expected baseline | Approval captures the exact head activation expects. |
| Compare-and-swap | Activation succeeds only while that baseline still matches. |
| Immutable state | Success creates one exact-definition state record. |
| Predecessor | Replacement names the state it supersedes. |
| Generation | Successful publication advances monotonically. |
| Replay | Exact retry returns the original state and identities. |
| Atomic publication | Runtime sees the complete previous or replacement state. |

Current state contains exactly one authority kind:

- `active-value`: one approved contract-valid value, no strategy identity;
- `numeric-rule`: one approved deterministic rule with a derived strategy
  identity.

Both kinds share validation, CAS, replay, lineage, and publication.

## Decision constraints

Contract Service validates candidates against the exact definition, target
hierarchy, output contract, fallback, and declared decision constraints.

Constraints may narrow authority but never widen it. There is no separate
Policy service or alternate approval path.

## Lifecycle records

Contract Store records the exact approved snapshot, actor, comment, expected
baseline, and readiness result. State Store records proposal, activation,
state, predecessor, generation, and strategy identities.

Together these records reconstruct approval, activation, retry, conflict, and
supersession without a separate Audit service.

## Runtime handoff

```text
ready definition projection
  + runtime target and live inputs
  + compatible active state
  + decision constraints
  -> RuntimeDecisionResult
```

The authority source is irrelevant to runtime. Bundle-approved and future
proposal-managed candidates converge on the same state boundary.

## Design invariants

1. Candidates never self-approve.
2. Contract Service derives trusted lifecycle identities.
3. Approval and activation are distinct durable events.
4. One stable head orders authority across revisions.
5. Activation uses the baseline captured during approval.
6. Exact replay returns the original publication.
7. Runtime reads authority but cannot create it.
8. Async analysis submits candidates through Contract Service.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Contract Service](../design/contract-service/README.md)
- [State Store](../design/state-store/README.md)
- [Async Analysis Pipeline](../design/async-analysis/README.md)
