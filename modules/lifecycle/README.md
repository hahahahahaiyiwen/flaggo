# Lifecycle Module

Owns the application workflow from an authenticated proposal to review,
explicit automatic approval, and audited activation. It does not generate
proposals or select values for individual runtime requests.

## Current implementation

`IProposalGovernance` exposes `ReviewAsync(LifecycleReviewRequest, ...)` and
`ActivateAsync(LifecycleActivationRequest, ...)`. `ProposalGovernance`
orchestrates registry-owned definition projections, lifecycle policy, scoped
proposal evidence, trusted actor resolution, and the state-owned atomic
lifecycle journal. There is no new HTTP endpoint or SDK wire contract.

The implemented proposal kinds are fixed primitive values and deterministic
numeric-rule strategies. Each proposal identifies one explicit control target,
the exact definition identity, an expected state ID/generation, producer
source, rationale, evidence/confidence references, and creation/expiry metadata.
An omitted target is not interpreted as global authority.

`ILifecycleActorProvider` supplies the authenticated subject, issuer, resource
scope, and review/activation permissions. Actors are not accepted in producer
requests. A producer's `operator`, `scripted`, `automated`, or `intelligence`
label is provenance, never authority.

## Review and activation

Review resolves both exact registry projections, current baseline, effective
policy context, and referenced evidence. `ILifecyclePolicyEvaluator` is separate
from runtime `IPolicyEvaluator`. Automatic approval must be explicitly permitted
by both environment policy and operator controls.

| Review disposition | Authority effect |
| --- | --- |
| `approved` | Persist a separately identified automatic approval; activation is still a separate operation. |
| `limited` | Return reasons and effective restrictions; do not clamp or activate the submitted proposal. |
| `pending-approval` | Record that human approval is required; no approval or activation is fabricated. |
| `hold` | Record insufficient evidence, timing, or pause; keep current authority. |
| `rejected` | Record incompatibility or denied policy/authority; keep current authority. |

Malformed requests and authentication, configuration, or dependency failures
raise explicit exceptions rather than success-shaped receipts. Re-review after
a hold or changed inputs uses a new review ID. A revised proposal needs a new
proposal ID; those IDs bind immutable content.

For a new activation, the service rechecks actor authority and reloads
definition, policy, evidence, and baseline snapshots. Changed inputs invalidate
the recorded approval as `review_stale`; expiry and current policy can also
block activation. The state transaction then enforces compare-and-swap on the
reviewed baseline and rechecks evidence expiry/age after acquiring the writer
lease. Evidence cannot age out while waiting and still grant new authority.
A pending or otherwise non-approved review cannot be
upgraded by calling activation.

Review records preserve the definition snapshot digest, exact policy context
and effective policy, resolved evidence, baseline, actor, and proposal. The
separate automatic approval binds proposal/review digests, policy revision,
initiating identity, `automatic` mode, and timestamp. It is not human approval.

## Atomicity and retries

`IGovernedStateLifecycleStore` co-commits review/approval records, lifecycle
audit, immutable operation receipts, and state. Audit semantics and
`ILifecycleAuditReader` remain owned by the audit module. Runtime gets only the
read-only state projection; it does not call lifecycle policy or receive a
mutation port.

An exact authorized retry returns the original immutable review or activation
receipt before resolving changing dependencies. Changed reuse conflicts.
Successful activation consumes the proposal once. Current state/history lookup
is separate: replaying an old activation does not reactivate a superseded state.

Commit waits default to five seconds and honor request cancellation and host
shutdown. An underlying non-cooperative writer keeps its lease until it really
finishes; late faults are observed and logged. Timeout, cancellation, or a lost
response can mean an unknown committed outcome, not proof of rollback. Retry
the original identity to reconcile; do not mint a replacement operation ID.

The file adapter publishes a complete version 3 journal/state snapshot through
the existing descriptor-last committed-artifact protocol. Pre-publication
failure cannot expose new authority. Readers reject incomplete lifecycle proof.
Old unaudited lifecycle formats are not migrated.

## Composition and boundaries

See the [control-plane configuration](../../apps/control-plane/README.md) for
authenticated actor mapping, policy contexts, proposal evidence, and the shared
state descriptor. One descriptor serves one application/environment scope.

Manual approval workflows/UI, proposal generation and operator/scripted entry
points, experiments/rollouts, and demo-bootstrap migration are not implemented
here. Issue #25 supplies producers over this boundary. Lifecycle
`maximumActivationDelta` and `minimumActivationIntervalSeconds` constrain
durable state changes; they do not implement per-request stabilization or
change the frozen runtime cooldown semantics owned by #33.

Focused behavior is covered by `ProposalGovernanceTests`,
`LifecycleHostingTests`, `LifecyclePolicyTests`, `LifecycleJournalTests`,
`ProposalEvidenceReaderTests`, and the governed-state persistence tests.
