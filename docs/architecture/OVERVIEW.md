# Flaggo architecture overview

## Purpose

Flaggo is a policy-first decisioning control plane with a runtime decision
provider. This document defines the shared vocabulary, layers, component map,
and control/data-plane boundaries. Detailed contracts live in the linked
architecture, component, and executable contract documents.

## System at a glance

The application delegates a bounded runtime variable. Flaggo makes the
authority for that variable explicit, applies it deterministically, records the
result durably, and attributes outcomes only after the application confirms it
used the result.

```text
control plane
  definition bundle
    -> validation
    -> authenticated approval
    -> governed-state activation

data plane
  exact definition identity + runtime target + live inputs
    -> compatible governed state
    -> active-value resolution or numeric-rule evaluation
    -> runtime policy
    -> durable audit
    -> RuntimeDecisionResult

application
  applies result
    -> confirms exposure
    -> receives exposureId
    -> emits exposure-linked outcomes
```

The current Phase 3 path starts from a bundle-authored authority candidate.
Future Phase 4 work may produce a bounded proposal from evidence or another
authorized source, but governance must still activate state before runtime can
consume it.

## Core vocabulary

| Concept | Primary question | Ownership |
| --- | --- | --- |
| Decision key | Which runtime decision family did application code delegate? | Stable developer-facing name such as `tetris.dropInterval`. |
| Decision definition | What may be decided for this semantic revision? | Versioned targets, signals, intent, inference inputs, output contract, policy, fallback, and authority workflow. |
| Runtime identity | Which exact registered contract does this request expect? | `{ definitionId, revision, contractDigest }`. |
| Decision evidence | What is known now or historically? | Context, live signal inputs, observations, evidence views, quality, provenance, decision records, exposures, and outcomes. |
| Authority candidate | What bounded behavior is requesting approval? | Bundle-declared initial authority now; a future `DecisionProposal` later. |
| Governed decision state | Which behavior is authorized for runtime use? | Immutable `active-value` or `numeric-rule` authority plus activation lineage. |
| Runtime decision execution | How is approved behavior applied to this request? | Deterministic active-value resolution, numeric-rule evaluation, policy, fallback, and audit. |
| Runtime decision result | What did this request receive? | Value, mode, fallback, targets, policy, confidence, decision ID, and audit ID. |
| Exposure | Did the application actually apply or render the result? | Server record created only after confirmation; identified by `exposureId`. |

`GovernedDecisionState` is not part of the decision definition. It is produced
by an approved control-plane workflow. `RuntimeDecisionResult` is not authority;
it is one audited data-plane outcome.

## Layers and boundaries

| Layer | Owns | Does not own |
| --- | --- | --- |
| Definition | Semantic contract, target hierarchy, signal roles, action space, policy, fallback, and authority workflow. | Telemetry history, approved state, or runtime results. |
| Evidence | Runtime facts, signals, observations, evidence views, quality, provenance, decisions, exposures, and outcomes. | Approval or active authority. |
| Authority | Authenticated approval, activation, stable-head concurrency, replay, supersession, and readiness. | Per-request value selection. |
| Runtime | Applying compatible authority to one request, runtime policy, fallback, audit, and result construction. | Proposal generation or state mutation. |
| Application | Supplying context and inputs, applying the result, confirming exposure, and emitting outcomes. | Granting itself authority. |

The separation keeps online execution bounded and keeps asynchronous reasoning
out of the request path.

## Authority and runtime flows

### Bundle-approved authority

```text
definition + initial authority candidate
  -> bundle validation
  -> authenticated exact-snapshot approval
  -> captured expected authority baseline
  -> stable-head compare-and-swap
  -> immutable GovernedDecisionState
  -> ready registration receipt
```

The bundle declares a candidate; it cannot declare trusted approval, proposal,
activation, strategy, or state identities. The server derives those identities
and reuses them on exact replay.

### Proposal-managed authority

```text
future authorized producer
  -> bounded DecisionProposal
  -> governance
  -> initial or replacement activation
  -> GovernedDecisionState
```

This is a Phase 4 extension point, not a current proposal or governance DTO.
Issue #25 owns its detailed design. Both authority sources must converge on the
same governed-state boundary unless a later contract explicitly extends it.

### Runtime execution

```text
exact registered identity
  + runtime target and context
  + live declared inputs
  + compatible governed state
  + runtime policy
  -> active value, numeric rule, or governed fallback
  -> durable audit
  -> RuntimeDecisionResult
```

Runtime cannot create durable authority. Any future request-time mechanism must
have a separately approved bounded contract with enforceable latency and
resource budgets; an unbounded agent loop never belongs in the request path.

## System components

Flaggo uses nine logical components. Repository modules and deployment units
may differ, but each responsibility has one owning boundary.

| # | Component | Responsibility | Detailed design |
| --- | --- | --- | --- |
| 1 | Client library | Code-first definitions, extraction, control-plane registration, typed runtime calls, exposure confirmation, and optional availability fallback. | [Client library](../design/client-library/README.md) |
| 2 | Decision API | Validate exact runtime requests, resolve dependencies, execute authority, apply policy, persist audit, and return results. | [Decision API](../design/decision-api/README.md) |
| 3 | Telemetry and evidence | Ingest observations and expose evidence/provenance boundaries. | [Telemetry and evidence](../design/telemetry-evidence/README.md) |
| 4 | Contract and registry | Validate and store canonical definitions, revisions, policies, and runtime projections. | [Contract and registry](../design/contract-registry/README.md) |
| 5 | Policy | Deterministically narrow candidates using definition and environment constraints. | [Policy](../design/policy/README.md) |
| 6 | State | Own stable authority heads, immutable states, activation lineage, replay, and runtime projections. | [State](../design/state/README.md) |
| 7 | Reasoning engine | Execute current numeric rules and preserve a separate future proposal-production seam. | [Reasoning engine](../design/reasoning-engine/README.md) |
| 8 | Audit and explanation | Durably record decisions, inputs, targets, state/policy lineage, fallback, and explanations. | [Audit and explanation](../design/audit-explanation/README.md) |
| 9 | Operator console | Inspect current definitions, state, and audit; future governed operator actions require explicit contracts. | [Operator console](../design/operator-console/README.md) |

Cross-cutting language-neutral contracts are owned by
[shared contracts](../design/shared-contracts/README.md), not by a tenth
component.

## Target vocabulary

A decision definition declares a hierarchy of target kinds. Each path resolves
a specific target role instead of overloading one generic "scope".

| Target role | Meaning | Tetris example |
| --- | --- | --- |
| Runtime target | Concrete entity receiving this decision now. | `session:game-456` |
| Learning target | Population selected for future analysis. | `cohort:new_players` |
| Control target | Boundary where authority is approved and stored. | `cohort:new_players` |
| Evidence target | Boundary used by an evidence view. | `cohort:new_players` over 24 hours |
| Fallback target | Broader permitted level used by resolution. | `global` |
| Policy scope | Boundary where a policy applies. | environment or decision target |

The target hierarchy authorizes kinds. Runtime resolution follows the exact
primary inference target and explicit `fallbackOrder`; it does not insert
undeclared levels or arbitrate overlapping selectors.

## Control plane, data plane, and deployment

| Boundary | Owns |
| --- | --- |
| Control plane | Bundle validation/apply, immutable definition publication, authenticated approval, activation, supersession, state, and registration readiness. |
| Data plane | Exact-identity decide, active-value resolution, numeric-rule evaluation, governed fallback, durable decision audit, and exposure confirmation. |
| Application deployment | Building and deploying application code; remains outside Flaggo ownership. |

For the MVP, trusted application/bootstrap startup acts as a control-plane
client. Browser code must not contain management credentials. Runtime decide
never registers or changes a definition.

```text
code-first declaration
  -> static extraction
  -> independent application deployment
  -> trusted control-plane registration and approval
  -> ready runtime binding
  -> data-plane decision calls
```

## Identity, state, and reuse

The runtime identity is always the complete tuple:

```text
{ definitionId, revision, contractDigest }
```

The stable authority address is:

```text
application + environment + decision key + control target
```

One stable head orders replacement across semantic revisions. Every governed
state is immutable and bound to the exact runtime identity that was approved.
Activation compares the stable head with the baseline captured during approval;
a changed head is a stale conflict, not an alternate mutation path.

| Layer | Reuse rule |
| --- | --- |
| Raw observations | Reusable when immutable signal semantics match. |
| Evidence views | Reusable when signal, target, window, and filters match. |
| Stable authority head | Shared across semantic revisions at one authority address. |
| Governed state | Never silently reused across exact runtime identities. |
| Decision, audit, exposure, and outcome records | Bound to the exact identity used by the request. |

## Failure, fallback, audit, and attribution

Missing, unknown, conflicting, retired, or non-ready definition identity is an
explicit contract or readiness error. It cannot become local fallback.

A valid registered decision may return governed server fallback when no
compatible authority exists or runtime policy prevents normal execution. An
optional SDK fallback is limited to recognized data-plane availability
failures and cannot claim a server decision, policy, audit, or exposure
identity.

A ready service must commit decision audit to storage that survives process
failure before returning a successful result. Console and in-memory sinks are
test or explicitly non-ready debug options only.

A returned decision is not proof of application:

```text
RuntimeDecisionResult
  -> application applies value
  -> confirm exposure
  -> exposureId
  -> exposure-linked outcome telemetry
```

Raw domain telemetry remains independent when it was not caused by an applied
decision.

## Current and future boundaries

Current authority kinds are exactly `active-value` and `numeric-rule`. Governed
fallback is a runtime outcome, not a persisted authority kind.

Experiment assignment, rollout routing, override, pause, rollback, expiry,
completion, cooldown, hysteresis, previous-result stabilization, and detailed
proposal-managed governance remain deferred until their contracts are
separately approved.

## Related documents

- [Documentation home](../README.md)
- [Manifesto](../MANIFESTO.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Evidence](EVIDENCE.md)
- [Authority](AUTHORITY.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Tetris scenario](../scenarios/TETRIS.md)
- [MVP implementation guide](../IMPLEMENTATION_GUIDE.md)
- [Component design index](../design/README.md)
- [Executable contracts](../../contracts/README.md)
