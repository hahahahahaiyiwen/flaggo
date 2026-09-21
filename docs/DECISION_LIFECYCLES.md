# Decision Lifecycles

## Purpose

Decision lifecycles are the control-plane workflows that turn proposed behavior into approved runtime authority and manage that authority until it is superseded, expired, completed, or rolled back.

They answer:

> How does a proposed optimization, experiment, or rollout become active, remain governed, and safely end?

Decision lifecycles do not generate deep recommendations and do not select values for individual application requests.

```text
Decision Intelligence, operator, or authorized automation
  -> DecisionProposal
  -> validation and governance
  -> GovernedDecisionState
  -> observation and lifecycle transitions

GovernedDecisionState
  -> Runtime Decision Execution
  -> RuntimeDecisionResult
```

## Boundary

| Capability | Owns | Does not own |
| --- | --- | --- |
| Decision intelligence | Evidence analysis and proposal generation. | Approval or active authority. |
| Decision lifecycle | Validation, approval, activation, monitoring, conclusion, and state transitions. | Per-request value selection. |
| Runtime execution | Applying compatible approved state to one request. | Proposing or approving future state. |

Proposals may originate from intelligence, operators, deployment automation, or another authorized producer. Authority comes from governance, not from the proposal source.

## Shared authority model

The lifecycle layer consumes:

- a versioned decision definition;
- a typed `DecisionProposal`;
- current and previous governed state;
- effective policy;
- target authority and conflict information;
- evidence quality and uncertainty;
- operator controls;
- current time and lifecycle history.

It produces either:

- a new or updated `GovernedDecisionState`;
- a pending approval disposition;
- a hold with no authority change;
- a rejection with reason codes;
- a transition to a replacement or previous known-safe state.

## Proposal and state lifecycles

```text
DecisionProposal:
  proposed -> validated -> pending-approval | approved | rejected

GovernedDecisionState:
  pending -> active -> superseded | expired | completed | rolled-back
```

Approval and activation are separate events. An approved proposal may remain pending until its start condition, schedule, or rollout prerequisite is satisfied.

Rollback is a transition, not a governed-state payload kind. It activates a replacement or previous known-safe state and marks the replaced state `rolled-back`.

The producer-facing boundary is `IProposalGovernance` in `modules/lifecycle`.
It accepts typed fixed-value or numeric-strategy proposals, resolves trusted
actor authority separately, and orchestrates review and explicit activation.
The expected baseline is a state ID/generation for one
application/environment/decision/control-target address. No omitted target
becomes implicit global authority.

The current slice prioritizes automatic approval. An approved review persists
a distinct approval bound to proposal/review digests, effective policy
revision, initiating actor, timestamp, and `automatic` mode. It is not human
approval. Human-required policy stays `pending-approval`; no manual approval
workflow/UI or producer entry point is implemented by this slice.

`IGovernedStateLifecycleStore` accepts trusted review/activation/terminal
commits, not arbitrary runtime state or unaudited producer writes. It checks
the recorded approval and expected baseline and atomically records approval,
audit, receipts, and state. Replacement increments generation and records the
predecessor, proposal, and activation identity. Before new activation, the
application boundary rechecks definition, actor, policy, evidence, and expiry.

Review and activation identities bind immutable operations. Exact authorized
replay returns the original receipt, including after expiry or supersession;
current state status is a separate history read. Changed reuse conflicts, and
successful activation consumes the proposal once. New review identities are
required after changed inputs or a hold.
Replay acknowledgement requires persistence-owned durability confirmation,
not just a visible receipt. The local adapter waits for any outstanding writer
and synchronizes the descriptor directory under the lease, including recovery
after a failed post-rename barrier. It never recreates a missing operation or
reevaluates changing dependencies to confirm an existing one.

The local adapter's version 3 committed artifact co-locates the lifecycle
journal and state history. Runtime rejects missing or inconsistent proof.
Descriptor-last publication exposes only a complete snapshot. Pre-publication
failure leaves old authority; timeout, cancellation, or a lost response after
publication may have committed. Retry the same identity rather than assuming
rollback. A bounded caller wait never releases an in-flight writer's lease.
Old unaudited lifecycle formats are not migrated; standalone demo bootstrap
remains separate.

## Governance

Governance evaluates proposals independently of the reasoning that produced them:

```text
DecisionProposal
  -> schema and definition compatibility
  -> workflow permission
  -> action-space validation
  -> target authority and conflict resolution
  -> evidence and uncertainty requirements
  -> safety and environment policy
  -> cooldown and blast-radius limits
  -> approval and operator controls
  -> activation, limitation, hold, rejection, or pending approval
```

Effective policy is the intersection of:

- definition-owned constraints;
- deployment or environment policy;
- operator controls.

Less-trusted layers may narrow behavior but cannot widen it.

The implementation uses a separate `ILifecyclePolicyEvaluator`; runtime
`IPolicyEvaluator` continues guarding individual requests against approved
state. Both environment policy and operator controls must explicitly allow
automatic approval. Blast-radius enforcement currently means authorized
target kinds/identities and one authority address per proposal, not traffic
allocation or population estimates. `maximumActivationDelta` and
`minimumActivationIntervalSeconds` apply to durable changes; runtime temporal
stabilization remains separate work.

## Governance dispositions

Proposal type and governance disposition are different concepts. An experiment is a proposal type, not an approval result.

| Disposition | Meaning |
| --- | --- |
| `approved` | The proposal may become active as submitted. |
| `limited` | Return restrictions; the submitted proposal cannot activate. A revised proposal requires another review. |
| `pending-approval` | Human or external approval is required. |
| `hold` | Existing authority remains unchanged because evidence or timing is insufficient. |
| `rejected` | The proposal violates contract, policy, authority, or safety requirements. |

## Governed state kinds

Approved authority may represent:

| Kind | Meaning |
| --- | --- |
| Fixed value | Return one approved value. |
| Strategy | Evaluate approved deterministic rules or bounded models. |
| Experiment | Allocate declared assignment targets among approved variants to generate comparative evidence. |
| Rollout | Progressively route an approved change across a population. |
| Fallback-only | Serve only the safe registered fallback. |
| Override | Apply explicit operator authority until removed or expired. |

These are the broader state kinds. The current lifecycle implementation
supports fixed values and numeric-rule strategies only. Runtime execution
mechanisms are defined separately in [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md).

## Adaptive optimization lifecycle

Adaptive optimization uses accumulated evidence to improve an active value or strategy:

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

## Experiment lifecycle

Experimentation intentionally creates controlled variation when evidence cannot yet identify a winner.

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

## Progressive rollout lifecycle

Rollout safely delivers a change that has already been selected:

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

## Operator intervention

Operators can:

- approve or reject pending proposals;
- pause or resume a lifecycle;
- limit experiment or rollout traffic;
- pin an override;
- force fallback-only state;
- conclude an experiment;
- promote a result;
- roll back active state.

Operator action must be explicit governed state or a recorded lifecycle transition, never a hidden exception.

## Audit and explanation

Lifecycle audit records should capture:

- proposal identity and source;
- definition identity;
- evidence references;
- effective policy and reason codes;
- requested and resolved targets;
- approval identity and timestamp;
- previous and replacement state IDs;
- experiment or rollout transition;
- operator actions;
- explanation and rollback rationale.

Every active state must be reconstructable from its proposal, policy outcome, approval, and transition history.

`ILifecycleAuditReader` exposes the current internal journal. Audit owns
record semantics and integrity checks; state owns the co-commit boundary.
Lifecycle audit is not appended to a separate sink before or after state
publication. Request-time decision/exposure audit remains independent.

## Relationship to contract and evidence flows

Contract registration and telemetry ingestion support lifecycles but are not decision lifecycles themselves:

```text
contract sync
  -> supplies immutable definition identity and permissions

telemetry ingestion
  -> supplies evidence and attributed outcomes

decision lifecycle
  -> creates and transitions governed authority

runtime execution
  -> consumes that authority
```

## Design principles

1. **Authority is explicit**: only approved governed state can affect runtime behavior.
2. **Proposal type is not disposition**: experiment and rollout describe proposed behavior; approved and rejected describe governance outcomes.
3. **Approval is separate from activation**: timing and prerequisites remain enforceable.
4. **Transitions are auditable**: promotion, supersession, completion, and rollback identify previous and replacement state.
5. **Policy cannot be bypassed**: intelligence and operators act through explicit governance mechanisms.
6. **Experiments require opt-in**: controlled variation must remain inside a definition-owned envelope.
7. **Rollout and experiment intent remain distinct**: risk reduction is not causal comparison.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Intelligence](DECISION_INTELLIGENCE.md)
- [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
