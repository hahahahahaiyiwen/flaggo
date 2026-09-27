# Decision authority

## Purpose

This document defines which records establish contract, executable, and
runtime authority. It separates three questions that the earlier architecture
combined:

1. Which contract was accepted?
2. Which executable was generated?
3. Which executable is active for runtime evaluation?

Only the third question selects runtime behavior.

## Authenticated scope

All management and runtime authority is interpreted within the authenticated
application/environment scope. That scope comes from credentials and server
authorization policy, not from contract attributes, route names, exposure
context, or user-provided identifiers.

Bearer tokens represent that scope with two required claims:

| Claim | Meaning |
| --- | --- |
| `flaggo_application` | Stable application identifier |
| `flaggo_environment` | Stable deployment-environment identifier |

Both claims must be non-empty strings. Authorization scopes are read from the
standard space-delimited `scope` claim or the `scp` claim. If both are present,
their values are combined. A token missing either Flaggo scope claim is not a
valid management or runtime authority identity.

The conceptual authority records are:

```text
AcceptedContract[scope, contractName, contractDigest]
  = immutable DecisionContract

ExecutableArtifact[scope, executableDigest]
  = immutable DecisionExecutable bound to contractDigest

ExecutableLifecycle[scope, contractDigest, executableDigest]
  = Candidate | Active | Inactive
```

`IExecutableStore` owns executable artifacts and their lifecycle state. The
digest-bearing artifact never changes. Activation atomically changes scoped
lifecycle rows so exactly one executable is `Active` for a scope and contract
digest. The conceptual projection remains:

```text
RuntimeActivation[scope, contractDigest]
  = the executableDigest whose lifecycle state is Active
```

The route's `contractName` verifies that the requested digest belongs to the
named resource. The digest, not the name alone, selects the exact runtime
contract version.

## Contract acceptance authority

Contract Acceptance validates, canonicalizes, hashes, and durably stores one
exact `DecisionContract`. Acceptance establishes:

- the named contract version exists in the authenticated scope;
- its semantic content is immutable under `contractDigest`;
- generated executables must conform to that exact content; and
- its required `result.default` may be used to generate the default
  executable.

Acceptance does not activate authored behavior. The contract may contain an
`authoredExecutable`, but that field is source material for generation rather
than active behavior.

Successful publication reports an accepted-ready version only after the
Contract Service has generated and activated its default executable. An
internal staging record that has not completed that transition is not exposed
as accepted-ready runtime authority.

The Contract Service may also maintain:

```text
ManagementCurrent[scope, contractName] = contractDigest
LearningHead[scope, contractName] = contractDigest
```

`ManagementCurrent` supports management `GET` operations. `LearningHead`
selects the version eligible for ongoing evidence-based analysis. Neither
pointer participates in runtime version resolution.

## Executable identity and lifecycle roles

`DecisionExecutable` is one immutable artifact:

```text
executableDigest = digest(canonical({
  contractDigest,
  kind,
  behavior
}))
```

The following terms describe source or lifecycle roles, not different runtime
primitives:

| Term | Authority meaning |
| --- | --- |
| `AuthoredExecutable` | User-authored source contained in the contract; no direct runtime authority |
| `DefaultExecutable` | Executable derived from the accepted literal default; activated automatically |
| `CandidateExecutable` | Valid generated executable whose scoped lifecycle state is `Candidate` |
| `ActiveExecutable` | Immutable executable whose scoped lifecycle state is `Active` |

Generation provenance does not alter executable identity. A candidate retains
the same `executableDigest` when it is activated.

## Activation policy

Candidate validation establishes whether an immutable executable conforms to
its exact accepted contract; activation establishes whether runtime uses it.
These are the only post-generation gates in the initial model.

Default and authored candidates activate automatically after their required
generation and validation steps. Evidence-generated candidates use
`DecisionContract.learning.policy`; the initial and only supported mode is:

```yaml
mode: auto-activation
```

For a valid evidence-generated candidate from the current learning head, the
Contract Service immediately attempts atomic activation. A failed validation,
superseded learning run, or failed activation leaves the existing executable
active.

`auto-activation` does not grant the learning worker authority to change
executable lifecycle state, and it does not tell the Decision Service to
discover the latest generated executable. The worker produces an immutable
candidate; the Contract Service performs the checked activation transition
through `IExecutableStore`.

## Activation

Activation is the sole operation that grants runtime executable authority:

```text
IExecutableStore.Activate(scope, contractDigest, executableDigest)
  -> prior Active becomes Inactive
  -> selected executable becomes Active
```

Before writing the mapping, the Contract Service verifies:

1. the contract digest is accepted in the same scope;
2. the executable exists and binds that exact digest;
3. the executable content matches its digest and conforms to the contract;
4. an evidence-generated candidate still belongs to the current learning
   head and has not been superseded.

The state replacement is atomic. A reader observes either the complete
previous active executable or the complete replacement. The immutable
executable fetched by an in-progress request remains valid for that request
even if activation changes after resolution.

Activation for one digest never changes another digest's mapping. Accepting a
new version therefore preserves runtime behavior for callers that still carry
the old digest.

## Concurrency and conflict handling

Candidate generation may race with another generation run, a newer activation,
or acceptance of a newer contract digest. The authority boundary resolves
those races before changing activation:

- only one complete executable digest can occupy a digest's activation slot;
- a superseded learning run cannot restore an older learning head;
- duplicate activation of the same executable is idempotent;
- a failed validation or activation attempt leaves the prior activation
  intact; and
- no operation assembles runtime behavior from multiple candidate versions.

The storage mechanism may use transactions, conditional writes, or another
atomic primitive. A generic target-addressed compare-and-swap API is not part
of the public decision model.

## Runtime resolution

For an exact-version request, the Decision Service resolves:

```text
(authenticated scope, contractName, contractDigest)
  -> accepted name/digest relationship
  -> IExecutableStore.GetActive(scope, contractDigest)
  -> immutable ActiveExecutable
```

It fails explicitly if any link is missing or inconsistent. It never:

- follows `ManagementCurrent` or `LearningHead`;
- substitutes another digest's executable;
- reconstructs a missing executable from the default;
- resolves a fallback target or parent hierarchy; or
- treats evidence or generation provenance as executable
  authority.

## Retired authority concepts

The target architecture does not use:

- definition-bundle authority as a separate runtime identity;
- target-specific stable heads or target parent chains;
- `active-value` and `numeric-rule` as authority strategies;
- a generic State Store record as the public source of decision authority;
- a separate public Activation Store or Activation Index;
- state generations or expected-head tokens in runtime requests; or
- mutable executable content.

The underlying implementation may use a database or transactional key-value
store, but those storage choices do not reintroduce the retired conceptual
model.

## Invariants

1. Accepted contract content is immutable under `contractDigest`.
2. Executable content is immutable under `executableDigest`.
3. Every executable binds exactly one accepted contract digest.
4. Authored source and generated candidates have no runtime authority by
   themselves.
5. Generation never implies runtime authority; activation does.
6. Activation is atomic and selects exactly one executable for a scope and
   contract digest.
7. Management current-version and learning-head pointers never select a
   runtime version.
8. A failed replacement leaves the existing active executable unchanged.
9. New contract versions preserve old-version activations.
10. Authentication, not decision data, establishes authority scope.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
- [Decision contracts and executables](../contracts/CONTRACTS.md)
