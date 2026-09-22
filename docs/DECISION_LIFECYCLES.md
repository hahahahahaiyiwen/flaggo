# Decision Lifecycles

## Purpose

Decision lifecycles are the control-plane workflows that turn declared or
proposed behavior into approved runtime authority and manage that authority
until it is superseded, expired, completed, or rolled back.

They answer:

> How does declared or proposed behavior become active, remain governed, and
> safely end?

Decision lifecycles do not generate deep recommendations and do not select values for individual application requests.

```text
Definition bundle
  -> initial authority candidate
  -> authenticated bundle approval
  -> activation
  -> GovernedDecisionState

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
| Decision lifecycle | Bundle approval, proposal governance, activation, monitoring, conclusion, and state transitions. | Per-request value selection. |
| Runtime execution | Applying compatible approved state to one request. | Proposing or approving future state. |

Initial authority may be declared in a definition bundle. Later proposals may
originate from intelligence, operators, deployment automation, or another
authorized producer. Authority comes from authenticated approval and lifecycle
activation, not from candidate or proposal source.

## Authority workflows

### Bundle-approved authority

The bundle declares a complete initial authority candidate for one definition:

```text
bundle apply
  -> validate definition and candidate
  -> approval request
  -> authenticated approval of the exact snapshot
  -> deterministic proposal, activation, and strategy identities
  -> expected-baseline compare-and-swap
  -> active governed state
  -> ready registration receipt
```

The bundle cannot provide trusted proposal, activation, strategy, state, or
approval identities. Exact retry resumes the same activation. Changed
authority requires a new semantic revision and approval. A stale expected
baseline conflicts at the stable authority head identified by application,
environment, decision key, and control target rather than replacing newer
authority through another revision namespace.

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
Activation derives the strategy identity in a third namespace from the
activation ID and canonical ID-free strategy declaration. Exact retry with the
same tuple returns the same identities; finding any ID bound to different
content is a conflict. The activation creates a state ID once, and replay
returns that original strategy ID, state ID, and generation.

An interrupted or outcome-unknown activation is retryable with the same
approval, proposal, and activation identities. Stale expected baseline,
changed authority content, or another permanent activation conflict remains
non-ready and requires bundle revalidation plus a new linked approval request;
it never overwrites newer authority or silently allocates another state.

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

Depending on the workflow, the lifecycle layer consumes:

- a versioned decision definition;
- a bundle-declared initial authority candidate or typed `DecisionProposal`;
- current governed state and expected baseline;
- effective policy;
- target authority and conflict information;
- evidence quality and uncertainty when a proposal claims them;
- operator controls and lifecycle history when the workflow requires them.

It produces either:

- a new or updated `GovernedDecisionState`;
- a pending approval disposition;
- a hold with no authority change;
- a rejection with reason codes;
- a transition to a replacement or previous known-safe state.

## Candidate, proposal, and state lifecycles

```text
Bundle initial authority:
  declared -> approval-pending -> authorized -> activated

DecisionProposal:
  proposed -> validated -> pending-approval | approved | rejected

GovernedDecisionState:
  pending -> active -> superseded | expired | completed | rolled-back
```

Approval and activation are separate events. Registration with required initial
authority is not ready until activation succeeds. An approved proposal may
remain pending until its start condition, schedule, or rollout prerequisite is
satisfied.

Rollback is a transition, not a governed-state payload kind. It activates a replacement or previous known-safe state and marks the replaced state `rolled-back`.

The shared activation core owns state identity and generation, authority
address, expected-baseline compare-and-swap, idempotent replay, predecessor and
approval references, validation conflicts, read-only runtime projection, and
atomic publication.

The merged Track B implementation is broader than this target. A follow-up
review will remove or defer generic proposal/source abstractions,
evidence/confidence/expiry metadata, completion/expiry/rollback transitions,
broad lifecycle statuses, and public transition/history APIs not required by
activation and replay.

## Governance

Proposal-managed governance evaluates proposals independently of the reasoning
that produced them:

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

## Governance dispositions

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

These are state kinds. Runtime execution mechanisms are defined separately in [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md).

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

Every active state must be reconstructable from either its bundle candidate and
bundle approval or its proposal, policy outcome, approval, and transition
history.

## Relationship to contract and evidence flows

Contract registration and telemetry ingestion support lifecycles but are not decision lifecycles themselves:

```text
contract sync and bundle approval
  -> supplies immutable definition identity and optional initial authority

telemetry ingestion
  -> supplies evidence and attributed outcomes

decision lifecycle
  -> creates and transitions governed authority

runtime execution
  -> consumes that authority
```

## Design principles

1. **Authority is explicit**: only approved governed state can affect runtime behavior.
2. **Bundles cannot self-approve**: a declared initial authority remains a candidate until authenticated approval and activation.
3. **Proposal type is not disposition**: experiment and rollout describe proposed behavior; approved and rejected describe governance outcomes.
4. **Approval is separate from activation**: timing and prerequisites remain enforceable.
5. **Transitions are auditable**: promotion, supersession, completion, and rollback identify previous and replacement state.
6. **Policy cannot be bypassed**: intelligence and operators act through explicit governance mechanisms.
7. **Experiments require opt-in**: controlled variation must remain inside a definition-owned envelope.
8. **Rollout and experiment intent remain distinct**: risk reduction is not causal comparison.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Intelligence](DECISION_INTELLIGENCE.md)
- [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
