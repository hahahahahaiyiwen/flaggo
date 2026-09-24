# Reasoning Module

This current internal library implements Decision Service orchestration:
registered definitions, active state, decision constraints, bounded strategy
execution, and durable Evidence Store collaborators. It is not a separate
reasoning service.

It depends only on constructor-injected module ports and returns shared domain
contracts to the hosting application.

## Current implementation

`src/Flaggo.Decisioning` implements fixed-value and deterministic numeric-rule
execution. It rejects
unknown or conflicting contract identities, returns the configured governed
fallback when no state exists, verifies value types, and records audit state
before returning a server result. The persisted audit includes the exact
returned reason so inspection does not reconstruct strategy reasoning.
Constructor-injected `ITargetResolver` and
`IStrategyExecutor` ports plus evidence-module
`IEvidenceProvider` and policy-module `IPolicyEvaluator` ports isolate all
cross-module collaboration. Numeric candidates are checked against action
bounds and step plus max delta, cooldown, evidence quality, uncertainty,
expected outcome, sample size, and pause constraints. A blocked candidate is
never returned; orchestration returns the governed fallback with null
confidence and explicit policy reasons.
When configured evidence constraints require evidence and none is available,
orchestration returns `required-evidence-unavailable` instead of producing a
server fallback. The definition's client-fallback policy determines whether
the Problem Details response permits SDK-local availability fallback.
Provider-reported evidence unavailability follows this same policy path, so
file I/O cannot bypass a fail-closed definition. A strategy or experiment
candidate without the confidence required by the frozen response contract is
blocked and converted to server fallback rather than serialized successfully.

For Phase 3, deterministic numeric rules can normalize and weight multiple
declared signal inputs before comparing the aggregate score with the governed
threshold. This keeps the Tetris rule explicit in activated state while the
executor remains application-neutral. Missing, nonnumeric, nonfinite, or
duplicate required rule inputs produce `invalid_strategy_input`.

Runtime responses expose compact confidence and provenance. Audit and pending
exposure snapshots retain the full evidence view. Cohort claims resolved by the
target adapter are marked `client-verified` when unchanged and
`server-replaced` when an authoritative mapping changes them. Broader target
selection records server-derived resolution fallback; global fallback retains
the originating cohort claim and records `global` as the resolved target only
when cohort was part of the permitted resolution path.
Unverified cohort claims are excluded from governed-state lookup. Direct
user, session, and custom context targets remain `client-claimed`; matching a
client-provided value does not make it authoritative.

Target resolution is definition-driven. Reasoning consumes the registry-owned
runtime projection, validates the request target and target-bearing context
against its hierarchy, uses a supplied permitted runtime target as the primary
lookup target (otherwise the declared inference target), and then probes only
the target kinds listed in `inference.fallbackOrder`, in that order.
Missing required context and inconsistent target bindings fail closed before
state lookup. Returned governed state must match one of the permitted
resolution targets; a store cannot widen the definition by returning an
undeclared control target. Runtime context contributes a target identifier
only when the registry projection explicitly marks that field with a target
type; conventional field names are not interpreted by reasoning.
The frozen v1 `resolutionFallbackUsed` flag identifies terminal global or
no-state resolution, while intermediate approved targets remain part of the
normal resolution chain. A primary global target is therefore not reported as
fallback and is attributed as server-derived when no client target supplied
it. The legacy targetless-global state representation is probed only after all
declared fallback targets so it cannot preempt their explicit order.

Runtime action bounds and policy are shared with the intelligence projection.
Evidence thresholds apply to adaptive `strategy` and `experiment` candidates;
governed `active-value` state still enforces non-evidence constraints without
requiring an evidence snapshot.

`ExposureConfirmationService` owns confirmed-exposure orchestration through
constructor-injected state and audit ports. It prepares a stable exposure
identity, durably records the exposure, and only then commits confirmation.
Audit failure therefore leaves the receipt pending, while retry reuses the
same exposure identity and remains valid after the initial clock-validation
window; a different observation conflicts. The audit sink deduplicates the
record. Accepted matching replay returns the stored confirmation without
calling audit again, while a changed observation still conflicts.
Request cancellation remains authoritative through preparation and durable
audit. Once that audit succeeds, the consistency transition no longer uses the
request-abort token: an injected post-audit policy gives commit a finite
five-second deadline linked to application shutdown. A request that disconnects
after audit therefore cannot strand an otherwise completable confirmation,
while a hung or shutting-down store leaves the prepared state retryable. The
store receives that internal bounded token, but the caller also enforces the
deadline and shutdown independently so a non-cooperative adapter cannot hold
the request open. The durable audit already exists when such a wait ends.
Retry reconciles either the still-prepared state or a possibly late successful
commit with the same decision/exposure identity; late failures are observed
and logged rather than becoming unobserved task exceptions.

Future candidate selection and proposal generation across experiments,
statistical methods, and approved AI-assisted strategies belongs to Async
Analysis Pipeline, not this online service boundary.

Async analysis produces candidates; it cannot approve or activate them.
Candidates must pass through Contract Service. Inputs and outputs use typed
domain contracts, and all evidence, state, and model collaborators are
constructor-injected interfaces.

Update this document when strategy execution or proposal semantics change.
