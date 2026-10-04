# Decision contract lifecycle

## Purpose

This document defines the lifecycle that turns decision contract code into
runtime decisions. The lifecycle has three stages:

1. **Contract Acceptance**
2. **Executable Generation**
3. **Runtime Evaluation**

These names distinguish ongoing work from the states and events between
stages. A contract becomes accepted, generation yields a candidate executable,
candidate validation establishes conformance, and activation makes that exact
executable available to runtime evaluation.

`Executable Generation` intentionally includes both user-authored and
evidence-based paths. `Runtime Evaluation` is narrower than "runtime decision
execution": it identifies the deterministic evaluation function while the
Decision Service remains responsible for request validation, active-executable
resolution, and response delivery around that function.

## Lifecycle overview

```text
DecisionContract
  -> [1. Contract Acceptance]
  -> DecisionContract (accepted)
  -> [2. Executable Generation]
  -> DefaultExecutable + optional authored or evidence-based candidates
  -> [Activation]
  -> ActiveExecutable
  -> [3. Runtime Evaluation] + RuntimeInput
  -> RuntimeDecision
```

Activation is always required: a candidate never becomes runtime authority
merely because it was produced or validated. Flaggo generates and activates
the default executable automatically from the accepted contract's required
`result.default`.

An implementation may compose several transitions into one deployment API,
but it must preserve their distinct outcomes. Public contract deployment
reports accepted and ready only after default generation and activation
succeed. Generation of a later candidate does not make that candidate active.

## Stage 1: Contract Acceptance

Contract Acceptance establishes the exact contract under which executables,
evidence, and runtime decisions operate.

```text
DecisionContract
  -> authenticate publisher
  -> validate contract
  -> canonicalize semantic content
  -> compute contractDigest
  -> persist exact accepted contract
  -> DecisionContract (accepted)
```

The digest commits to the decision name and canonical semantic contract:

```text
contractDigest = digest(canonical(DecisionContract))
```

Acceptance does not determine whether another contract digest is compatible.
It validates the required default but does not produce runtime authority by
itself. The deployment workflow immediately continues through default
generation and activation before returning accepted readiness.

### Acceptance outcomes

| Outcome | Meaning |
| --- | --- |
| Accepted | The exact contract is durable, its default executable is active, and the digest is runtime-ready. |
| Idempotent acceptance | The same canonical named contract resolves to the existing digest and accepted record. |
| Rejected | Authentication, syntax, schema, semantic validation, or persistence failed; no accepted contract is reported. |

### New-digest cutover

When a changed contract under the same decision name produces `D2` while `D1`
already exists:

```text
RuntimeActivation[scope, D1] = E1
ManagementCurrent[scope, name] = D1

accept D2

RuntimeActivation[scope, D1] = E1
RuntimeActivation[scope, D2] = DefaultExecutable(D2)
ManagementCurrent[scope, name] = D2
```

Acceptance of `D2` preserves the `D1` activation for applications still using
`D1`. It moves the decision's current pointer to `D2`, supersedes ongoing `D1`
analysis, and permits `D2` analysis to start when evidence-based learning is
enabled.

Stopping old-digest analysis does not delete its evidence, deactivate its
executable, or prevent old application versions from requesting decisions.

## Stage 2: Executable Generation

Executable Generation transforms an accepted contract plus generation-specific
inputs into an immutable internal candidate executable.

### Default path

```text
DecisionContract (accepted).result.default
  -> generate bounded constant executable
  -> validate against exact contractDigest
  -> activate automatically
  -> DefaultExecutable
```

Every accepted contract follows this path. It requires no separate executable
authorization because its exact value was accepted as part of the contract.
The deployment workflow does not report ready until default activation
succeeds.

### Authored path

```text
DecisionContract (accepted).authoredExecutable
  -> compile deterministic expressions or synthesize condition-only rules
  -> validate against exact contractDigest
  -> CandidateExecutable
```

`AuthoredExecutable` is the optional user executable representation contained
in the accepted contract. Its syntax and shape may differ from Flaggo's
internal candidate representation. Generation and validation must not silently
change the accepted contract. Runtime candidates are always deterministic even
when their authored source contains natural-language conditions.

An authored executable replaces the already active default executable after
successful generation, validation, and automatic activation.

### Evidence-based path

```text
DecisionContract (accepted).learning
  + Evidence for the same contractDigest
  + evaluate.interval elapsed
  -> bounded asynchronous analysis
  -> validate against exact contractDigest
  -> CandidateExecutable
```

Evidence-based generation may be long-running, agentic, or nondeterministic.
It remains outside the request path. Its output is an immutable candidate that
must satisfy the same contract as an authored candidate.

Evidence-based analysis is scoped to the current contract. If the current
pointer moves before a run completes, the result may be retained for
reconstruction, but it is superseded and cannot activate. This check fences
late analysis results from restoring obsolete authority.

Only one analysis run may be active for a contract digest. The first run
becomes eligible one `learning.policy.evaluate.interval` after accepted
readiness. Each later interval starts when the previous attempt completes.
Completion may activate a candidate, produce no candidate, or record failure;
runtime evaluation continues in every case.

### Common generated output

The source of a candidate does not change the downstream lifecycle:

```text
DecisionContract.result.default ----------> generate ---\
DecisionContract.authoredExecutable ------> generate ----+-> CandidateExecutable
DecisionContract.learning + Evidence -----> analyze -----/
```

Every candidate records enough provenance to identify:

- its exact contract digest;
- whether it came from the default, authored, or evidence-based path;
- the compiler or analysis method that produced it;
- evidence references used by analysis, when applicable; and
- validation results.

A candidate produced for one contract digest cannot activate for another.

## Validation and activation gate

Candidate validation and activation form the transition from executable
generation to runtime evaluation.

```text
CandidateExecutable
  -> validate candidate identity and contract conformance
  -> verify candidate is current and eligible
  -> apply activation policy
  -> atomically activate for authenticated scope and contractDigest
  -> ActiveExecutable
```

### Automatic activation

Candidate validation and activation are the only post-generation gates in the
initial model. Default and authored candidates activate automatically after
their required generation and validation steps. Evidence-based candidates use
`DecisionContract.learning.policy`, whose initial and only supported mode is:

```text
mode = auto-activation
```

After candidate validation, this policy tells the Contract Service to attempt
activation immediately. It does not allow the analysis pipeline to write
runtime authority directly or bypass contract conformance, current-contract, or
activation conflict checks.

`auto-activation` does not mean that runtime discovers the latest generated
artifact. Candidate production and activation remain separate persisted
events, and runtime reads only `RuntimeActivation`.

### Activation

Activation is mandatory and atomically establishes:

```text
RuntimeActivation[authenticated scope, contractDigest] = ActiveExecutable
```

Activating a replacement for one digest does not modify activations for other
digests. Runtime observes either the previous complete activation or the new
complete activation, never a partial transition.

An accepted digest always has its default executable active. If an authored or
evidence-based candidate is absent, fails validation, is superseded, or cannot
activate, the current executable remains active. A missing activation for an
accepted-ready digest indicates corrupt or unavailable authority and fails
explicitly.

## Stage 3: Runtime Evaluation

Runtime Evaluation applies the exact active executable to explicit runtime
input:

```text
authenticated scope + contractDigest
  -> resolve exact ActiveExecutable

ActiveExecutable + RuntimeInput
  -> deterministic bounded evaluation
  -> RuntimeDecision
```

The same active executable and runtime input produce the same runtime decision.
Any value that may affect the result, including time or caller state, must be
represented explicitly in the runtime input or immutable executable rather
than read from ambient state.

The surrounding Decision Service:

1. authenticates tenant, application, and environment scope;
2. validates the contract digest and SDK-constructed complete runtime input;
3. resolves the exact active executable;
4. evaluates it without invoking executable generation;
5. validates the result against the accepted contract; and
6. returns the runtime decision without retaining per-request session state.

An unknown digest, missing activation, invalid input, evaluation failure, or
invalid result is explicit. Runtime never selects an implicit latest digest,
substitutes another digest's executable, or starts asynchronous analysis.

## Continuous Learning Loop

Contract Acceptance occurs once per digest, but Executable Generation and
Runtime Evaluation may form a continuous asynchronous feedback loop:

```text
ActiveExecutable -> Runtime Evaluation -> RuntimeDecision
       ^                                     |
       |                                     v
       |                      decision receipt + app telemetry
       |                                     |
       |                                     v
       +-- atomic activation <- valid candidate <- Async Analysis
                                                     ^
                                                     |
                                          materialized observations
```

Runtime and learning progress independently:

- Runtime Evaluation continuously serves the current `ActiveExecutable`.
- Decision observations and selected application telemetry accumulate without
  blocking runtime requests.
- When `learning.policy.evaluate.interval` elapses, one asynchronous analysis
  run becomes eligible.
- A valid candidate under `auto-activation` immediately attempts atomic
  activation.
- A runtime request observes either the complete previous executable or the
  complete replacement.
- Analysis failure, no-candidate completion, or activation conflict leaves the
  current executable active and begins the next waiting interval.

The loop begins with the automatically activated default executable, so a
learning-only contract can produce decisions and correlated application
evidence before its first learned candidate exists.

If a newer contract digest becomes current, the old loop is superseded. An
in-flight old-digest run may finish for reconstruction but cannot activate
afterward. Runtime traffic carrying the old digest continues using its existing
active executable.

## Lifecycle state summary

| State | Entered by | Permitted next transitions |
| --- | --- | --- |
| Submitted contract | User deployment | Reject or accept contract |
| Accepted contract | Successful deployment, including default activation | Evaluate with the default or produce replacement candidates |
| Waiting for learning interval | Accepted contract with learning enabled or completed analysis attempt | Start one analysis run when the interval elapses |
| Analyzing evidence | Eligible interval with no other run active | Produce no candidate, fail, or produce one candidate |
| Candidate executable | Successful Executable Generation | Fail validation, become superseded, fail activation, or activate when eligible |
| Active executable | Successful activation | Evaluate runtime requests or replace with another eligible candidate for the same digest |
| Superseded analysis | Current contract changed before completion | Retain for reconstruction; never activate |

Rejection, failure, cancellation, and supersession are completed outcomes, not
successful progression to the next stage.

## Cross-digest behavior

The lifecycle is isolated by contract digest except for the user-directed
current-contract transition:

```text
D1: accepted contract -> RuntimeActivation[scope, D1] = E1 -> runtime continues
D2: accepted contract -> RuntimeActivation[scope, D2] = DefaultExecutable(D2)
                    \-> authored or learned candidate -> replacement activation

ManagementCurrent[scope, name]: D1 -> D2
```

Flaggo does not infer executable or evidence compatibility between `D1` and
`D2`. Evidence remains attached to the digest under which it was observed.
Intentional reuse or transformation of old evidence belongs to the analysis
pipeline and must be recorded in candidate provenance.

## Invariants

1. Contract Acceptance validates a required contract default.
2. Successful accepted readiness includes automatic default generation and
   activation.
3. Every candidate is bound to one accepted contract digest.
4. Authored compilation and evidence-based analysis are generation mechanisms
   within the same lifecycle stage.
5. Activation is mandatory before runtime evaluation.
6. Candidate generation and validation never imply runtime authority.
7. Runtime resolves only the active executable for the authenticated scope and
   requested digest.
8. A new digest preserves older digest activations.
9. Moving the current contract supersedes old-digest analysis but not
   old-digest runtime execution.
10. Runtime evaluation is deterministic and never invokes executable
    generation.
11. Learning runs are single-flight per contract digest and start no more
    frequently than the configured evaluation interval.
12. Automatic activation never bypasses candidate validation, atomic
    replacement, or current-contract checks.

## Related documents

- [Application boundaries and lifecycle](../architecture/APP_BOUNDARIES.md)
- [Decision contracts and executables](CONTRACTS.md)
- [Runtime evaluation model](RUNTIME_EVALUATION.md)
- [Decision authority](../architecture/AUTHORITY.md)
- [Runtime client and Decision Service](../architecture/RUNTIME.md)