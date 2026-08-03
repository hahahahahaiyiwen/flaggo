# Reasoning Module

Owns online decision orchestration over exact registered definitions,
governed state, policy, evidence, strategy execution, and audit collaborators.
It depends only on constructor-injected module ports and returns shared domain
contracts to the hosting application.

## Current implementation

`src/Flaggo.Decisioning` implements the fixed-value vertical slice. It rejects
unknown or conflicting contract identities, returns the configured governed
fallback when no state exists, verifies value types, and records audit state
before returning a server result. It enforces registered signal declarations,
types and ranges, rejects retired revisions, resolves target provenance, and
writes the same immutable attribution snapshot to audit and pending exposure
state. Unverified cohort membership remains `client-claimed`; targetless active
decisions omit control targets and do not create exposure confirmation state.
When lookup selects a broader governed target, responses and attribution
snapshots record resolution fallback and server-derived provenance.

Future slices add target resolution, evidence, strategy execution, and policy
evaluation without moving infrastructure calls into this module.

Owns candidate selection and proposal generation across deterministic rules,
experiments, statistical methods, and approved AI-assisted strategies.

Reasoning produces proposals or candidate values; it cannot approve, activate,
or roll them out. Inputs and outputs use typed domain contracts, and all
evidence, state, and model collaborators are constructor-injected interfaces.

Update this document when strategy execution or proposal semantics change.
