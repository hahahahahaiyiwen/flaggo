# Decision Lifecycles

## Purpose

Decision lifecycles are the control-plane workflows that turn declared
behavior into approved runtime authority. Phase 3 activates bundle-approved
authority and supersedes it when replacement authority is activated. Expiry,
completion, rollback, and proposal-managed lifecycle transitions require
separately approved future contracts.

They answer:

> How does declared behavior become active, remain governed, and become
> superseded by replacement authority?

Decision lifecycles do not generate deep recommendations and do not select values for individual application requests.

```text
Definition bundle
  -> initial authority candidate
  -> authenticated bundle approval
  -> activation
  -> GovernedDecisionState

Future Phase 4:
Decision Intelligence, operator, or authorized automation
  -> DecisionProposal -> governance -> activation
  -> GovernedDecisionState

GovernedDecisionState
  -> Runtime Decision Execution
  -> RuntimeDecisionResult
```

## Boundary

| Capability | Owns | Does not own |
| --- | --- | --- |
| Decision intelligence | Evidence analysis and proposal generation. | Approval or active authority. |
| Decision lifecycle | Phase 3 bundle approval, activation, and supersession; future contracts may add proposal governance and broader transitions. | Per-request value selection. |
| Runtime execution | Applying compatible approved state to one request. | Proposing or approving future state. |

Initial authority may be declared in a definition bundle. Future
proposal-managed authority may originate from intelligence, operators,
deployment automation, or another authorized producer. Authority always comes
from authenticated approval and lifecycle activation, not from candidate or
proposal source.

## Authority workflows

### Bundle-approved authority

The bundle declares a complete initial authority candidate for one definition:

```text
bundle apply
  -> validate definition and candidate
  -> approval request
  -> authenticated approval of the exact snapshot
  -> atomically persist allocated identities and captured authority-head baseline
  -> deterministic proposal, activation, and strategy identities
  -> expected-baseline compare-and-swap
  -> active governed state
  -> ready registration receipt
```

The bundle cannot provide trusted proposal, activation, strategy, state, or
approval identities. Exact retry resumes the same activation. Changed
authority content requires a new semantic revision and approval. A stale
expected baseline conflicts at the stable authority head identified by
application, environment, decision key, and control target rather than
replacing newer authority through another revision namespace.

For each definition in an approved bundle, the server derives proposal and
activation identities from one canonical tuple:

- application and environment;
- approval request ID;
- decision key;
- contract digest;
- canonical initial authority, including control target, kind, rule, and
  rationale.

Proposal and activation identities use distinct namespaces over that tuple.
The decision key and canonical authority content make the identities unique per
definition in a multi-definition approval. The IDs remain opaque to clients.
At the same approval transition, the control plane reads each stable authority
head once and persists that exact expected baseline with its activation ID.
Every retry reuses the stored baseline; it never substitutes the head visible
at retry time.
Activation derives the strategy identity in a third namespace from the
activation ID and canonical ID-free strategy declaration. Exact retry with the
same tuple returns the same identities; finding any ID bound to different
content is a conflict. The activation creates a state ID once, and replay
returns that original strategy ID, state ID, and generation.

An interrupted or outcome-unknown activation is retryable with the same
approval, proposal, and activation identities. Stale expected baseline,
changed authority content, or another permanent activation conflict remains
non-ready and requires bundle revalidation plus a new linked approval request;
it never overwrites newer authority or silently allocates another state. When
the accepted bundle content is unchanged, that successor is an
authority-reauthorization: it reuses the published definition revision and
successful partial activations, includes only the failed authorities, captures
their new expected baselines, and receives new approval and activation
identities. Concurrent exact reapplies converge on the same successor.

Bundle-authored deterministic rules do not require model evidence or confidence.
Their authored rationale and approval actor remain lifecycle provenance.

### Proposal-managed authority

Phase 4 adds independently generated candidates:

```text
definition + evidence + current state
  -> authorized producer
  -> DecisionProposal
  -> governance
  -> replacement activation
```

The producer cannot write runtime authority directly.

## Shared authority model

The Phase 3 lifecycle consumes:

- a versioned decision definition;
- a bundle-declared initial authority candidate;
- current governed state and expected baseline;
- effective policy;
- target authority and conflict information.

It produces either:

- an active `GovernedDecisionState` that supersedes its predecessor;
- a pending approval disposition;
- a rejected approval or failed activation with reason codes.

Future proposal-managed contracts may additionally consume typed proposals,
evidence, uncertainty, operator controls, and lifecycle history. They may add
hold or broader transition outcomes without changing the Phase 3 state
contract implicitly.

## Candidate, proposal, and state lifecycles

```text
Bundle initial authority:
  declared -> approval-pending -> authorized -> activated

GovernedDecisionState:
  active -> superseded

Future DecisionProposal:
  proposed -> validated -> pending-approval | approved | rejected

Future lifecycle transitions:
  active -> expired | completed | rolled-back
```

Approval and activation are separate events. Registration with required initial
authority is not ready until activation succeeds.

Future proposal-managed approvals may remain pending until an approved start
condition or schedule is satisfied. Rollback is also a future transition
contract: it would activate replacement or previous known-safe authority rather
than becoming a governed-state payload kind.

The shared activation core owns state identity and generation, authority
address, expected-baseline compare-and-swap, idempotent replay, predecessor and
approval references, validation conflicts, read-only runtime projection, and
atomic publication.

The Phase 3 contract intentionally excludes generic proposal/source
abstractions, evidence/confidence/expiry metadata,
completion/expiry/rollback transitions, broad lifecycle statuses, and public
transition/history APIs not required by activation and replay.

## Future proposal-managed governance

The following is a conceptual Phase 4 boundary, not a current wire or state
contract. Proposal-managed governance would evaluate proposals independently
of the reasoning that produced them:

```text
DecisionProposal
  -> schema and definition compatibility
  -> workflow permission
  -> action-space validation
  -> target authority and conflict resolution
  -> evidence and uncertainty requirements
  -> safety and environment policy
  -> approved temporal and blast-radius limits when their contracts exist
  -> approval and operator controls
  -> activation, limitation, hold, rejection, or pending approval
```

Effective policy is the intersection of:

- definition-owned constraints;
- deployment or environment policy;
- operator controls.

Less-trusted layers may narrow behavior but cannot widen it.

## Future governance dispositions

Proposal type and governance disposition are different concepts. An experiment is a proposal type, not an approval result.
The following terms are conceptual Phase 4 outcomes, not a frozen wire enum;
issue #25 owns the concrete proposal and governance contract.

| Disposition | Meaning |
| --- | --- |
| `approved` | The proposal may become active as submitted. |
| `limited` | A constrained version may become active with reduced scope, traffic, delta, or duration. |
| `pending-approval` | Human or external approval is required. |
| `hold` | Existing authority remains unchanged because evidence or timing is insufficient. |
| `rejected` | The proposal violates contract, policy, authority, or safety requirements. |

## Current Phase 3 authority kinds

Approved Phase 3 authority represents exactly one of:

| Kind | Meaning |
| --- | --- |
| `active-value` | Return one approved value. |
| `numeric-rule` | Evaluate the approved deterministic numeric rule against declared inputs. |

Governed fallback is a runtime outcome, not a persisted authority kind.
Experiment, rollout, fallback-only, and override authority remain future
concepts pending separately approved state, lifecycle, runtime, and policy
contracts. Runtime execution mechanisms are defined separately in
[Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md).

## Future adaptive optimization lifecycle

The following is conceptual Phase 4 behavior, not a current contract. Adaptive
optimization may use accumulated evidence to improve an active value or
strategy:

```text
observe attributed outcomes and drift
  -> generate value or strategy proposal
  -> validate evidence and policy
  -> approve and activate governed state
  -> execute at runtime
  -> observe outcomes
  -> retain, revise, hold, or roll back
```

Optimization changes behavior because existing evidence supports a better candidate. It does not intentionally split comparable targets merely to create evidence.

## Future experiment lifecycle

Experimentation intentionally creates controlled variation when evidence cannot yet identify a winner.

This section is a conceptual Phase 4 direction, not a current contract.
Experiment pause, resume, duration, allocation changes, and other temporal or
operator transitions remain unavailable until separately approved lifecycle
and concurrency contracts define them.

### Definition permission

A decision definition must explicitly permit experimentation. The permission envelope should constrain:

- eligible assignment target kinds;
- allowed values or action space;
- maximum traffic;
- maximum variant count;
- required exposure confirmation;
- required success and guardrail signals;
- approval and safety requirements.

Active variants and allocation do not belong in the definition.

### Active experiment state

Governed experiment state should identify:

- experiment ID;
- hypothesis;
- control and treatment variants;
- allocation weights;
- assignment target kind;
- allocation version and salt;
- eligibility rules;
- success metrics and guardrails;
- start, review, and end conditions;
- lifecycle status;
- promotion and rollback information.

### Lifecycle

```text
propose experiment
  -> validate definition permission and candidate bounds
  -> approve hypothesis, variants, allocation, metrics, and duration
  -> activate experiment state
  -> runtime performs deterministic variant assignment
  -> confirmed exposures and outcomes accumulate
  -> analyze evidence
  -> continue, limit, pause, conclude, promote, or roll back
```

Promotion creates replacement governed state, commonly a fixed value or strategy. Stopping an experiment without a winner restores or retains the previous safe authority.

Experiment analysis may be performed by decision intelligence, an external statistical service, or an operator. Governance owns the resulting transition.

## Future progressive rollout lifecycle

A future rollout contract may safely deliver a change that has already been
selected:

```text
propose selected value or strategy
  -> approve rollout stages and guardrails
  -> activate initial stage
  -> runtime routes eligible targets
  -> observe guardrails
  -> advance, pause, complete, or roll back
```

Rollout and experimentation may both divide traffic, but their intent differs:

- an experiment splits traffic to compare alternatives;
- a rollout stages delivery to reduce operational risk.

Rollout state should identify stages, eligibility, current allocation, advancement criteria, pause conditions, and rollback authority.

## Future operator intervention

Under a separately approved operator-control contract, operators may:

- approve or reject pending proposals;
- pause or resume a lifecycle;
- limit experiment or rollout traffic;
- pin an override;
- force fallback-only state;
- conclude an experiment;
- promote a result;
- roll back active state.

Phase 3 exposes none of these pause, override, rollout, or rollback authoring
surfaces. When introduced, operator action must be explicit governed state or a
recorded lifecycle transition, never a hidden exception.

## Audit and explanation

Phase 3 lifecycle audit records should capture:

- bundle candidate and server-derived proposal identity;
- definition identity;
- effective policy and activation reason codes;
- requested and resolved targets;
- approval identity and timestamp;
- previous and replacement state IDs;
- activation explanation.

Future proposal-managed audit contracts may add evidence references, experiment
or rollout transitions, operator actions, and rollback rationale.

Every Phase 3 active state must be reconstructable from its bundle candidate,
bundle approval, and activation record. A future proposal-managed state must
likewise be reconstructable from its proposal, policy outcome, approval, and
transition history.

## Relationship to contract and evidence flows

Contract registration and telemetry ingestion support lifecycles but are not decision lifecycles themselves:

```text
contract sync and bundle approval
  -> supplies immutable definition identity and optional initial authority

telemetry ingestion
  -> supplies evidence and attributed outcomes

decision lifecycle
  -> activates and supersedes Phase 3 governed authority

runtime execution
  -> consumes that authority
```

## Design principles

1. **Authority is explicit**: only approved governed state can affect runtime behavior.
2. **Bundles cannot self-approve**: a declared initial authority remains a candidate until authenticated approval and activation.
3. **Current authority kinds are narrow**: Phase 3 persists only `active-value` or `numeric-rule` authority.
4. **Approval is separate from activation**: Phase 3 readiness requires both.
5. **Transitions are auditable**: Phase 3 supersession identifies previous and replacement state; future transition kinds require their own contracts.
6. **Policy cannot be bypassed**: intelligence and operators act through explicit governance mechanisms.
7. **Future experiments require opt-in**: controlled variation must remain inside a definition-owned envelope.
8. **Future rollout and experiment intent remain distinct**: risk reduction is not causal comparison.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Intelligence](DECISION_INTELLIGENCE.md)
- [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
