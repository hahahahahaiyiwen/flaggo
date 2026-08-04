# Reasoning Module

Owns online decision orchestration over exact registered definitions,
governed state, policy, evidence, strategy execution, and audit collaborators.
It depends only on constructor-injected module ports and returns shared domain
contracts to the hosting application.

## Current implementation

`src/Flaggo.Decisioning` implements fixed-value and deterministic numeric-rule
execution. It rejects
unknown or conflicting contract identities, returns the configured governed
fallback when no state exists, verifies value types, and records audit state
before returning a server result. Constructor-injected `ITargetResolver`,
`IEvidenceProvider`, `IStrategyExecutor`, and `IPolicyEvaluator` ports isolate
all cross-module collaboration. Numeric candidates are checked against action
bounds and step plus max delta, cooldown, evidence quality, uncertainty,
expected outcome, sample size, and pause constraints. A blocked candidate is
never returned; orchestration returns the governed fallback with null
confidence and explicit policy reasons.

Runtime responses expose compact confidence and provenance. Audit and pending
exposure snapshots retain the full evidence view. Cohort claims resolved by the
target adapter are marked `client-verified` when unchanged and
`server-replaced` when an authoritative mapping changes them. Broader target
selection records server-derived resolution fallback.

Owns candidate selection and proposal generation across deterministic rules,
experiments, statistical methods, and approved AI-assisted strategies.

Reasoning produces proposals or candidate values; it cannot approve, activate,
or roll them out. Inputs and outputs use typed domain contracts, and all
evidence, state, and model collaborators are constructor-injected interfaces.

Update this document when strategy execution or proposal semantics change.
