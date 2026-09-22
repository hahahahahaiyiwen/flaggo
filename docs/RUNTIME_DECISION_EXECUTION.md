# Runtime Decision Execution

## Purpose

Runtime decision execution is the data-plane capability that applies compatible approved state to one application request.

It answers:

> Given this decision definition, runtime target, request context, policy, and active governed state, what value should this request receive?

Runtime execution does not decide whether an experiment or rollout should exist, analyze which candidate is globally best, or mutate lifecycle authority.

```text
DecisionDefinition
  + runtime target and context
  + compatible GovernedDecisionState
  + runtime policy
  -> Runtime Decision Execution
  -> RuntimeDecisionResult
```

## Runtime invariants

Runtime execution must be:

- deterministic for the same registered identity, state, target, and inputs;
- bounded by the definition action space and active state;
- fast enough for the application request path;
- safe when evidence or state is unavailable;
- auditable;
- consistent across service replicas;
- unable to create new durable authority.

For the MVP, runtime reasoning without compatible `GovernedDecisionState` is prohibited.

## Authority-source independence

Runtime execution does not care whether compatible state originated from an
approved bundle candidate or an independently generated proposal. Both sources
must produce the same governed-state shape and pass the same definition,
target, output, and runtime-policy checks.

A bundle-authored deterministic rule does not claim learned evidence or model
confidence. Evidence and confidence are present only when the approved
authority and applicable policy require them.

## Request flow

```text
Application
  -> Decision API
  -> validate exact definition identity
  -> validate registration and state-store readiness
  -> resolve ordered exact targets from inference target + fallbackOrder
  -> read stable authority heads in that order
  -> select the first exact definition-compatible state
  -> verify state compatibility and policy
  -> execute fixed value, strategy, variant assignment,
     rollout routing, override, or fallback
  -> validate returned value
  -> record decision inputs, state/policy lineage, and applicable evidence
  -> RuntimeDecisionResult
```

Missing, unknown, conflicting, or retired definition identity is a contract error. It must not silently become fallback.

## Execution mechanisms

| Mechanism | Runtime responsibility |
| --- | --- |
| Fixed resolution | Return the approved active value. |
| Strategy evaluation | Evaluate approved deterministic rules or bounded models against declared inputs. |
| Variant assignment | Deterministically assign an eligible target to an approved experiment variant. |
| Rollout routing | Route an eligible target according to the current approved rollout stage. |
| Override | Return the applicable operator-pinned value. |
| Fallback | Return the registered safe value when no permitted target has compatible active state, or applicable evidence/runtime policy prevents normal execution. |

The mechanism is selected from governed state; the runtime does not choose a new lifecycle.

## State and target resolution

```text
runtime target + verified context + explicit fallbackOrder
  -> ordered exact resolutionTargets
  -> stable authority head for each target
  -> first active state matching the request's exact definition identity
```

The target hierarchy authorizes target kinds but never inserts undeclared
fallback levels. A well-formed head for another revision is simply
incompatible and resolution may continue. Corrupt or incoherent state, a torn
store, or a non-ready registration is a readiness error; runtime must not turn
those failures into `missing_state` fallback.

The decision record should distinguish:

- runtime target receiving the decision;
- control target owning the active state;
- evidence target or views used by a strategy;
- fallback target when fallback resolves elsewhere.

## Fixed resolution

Fixed state returns one approved value after compatibility and policy checks:

```text
active fixed value
  -> validate output type, range, and allowed values
  -> return value
```

## Strategy evaluation

A strategy is an approved bounded plan for request-time evaluation.

Supported forms may include:

- rule tables;
- scoring functions;
- small deterministic models;
- approved bandit policies;
- explicitly authorized bounded AI inference.

```text
active strategy
  + declared inference inputs
  + permitted fresh evidence when required
  -> evaluate
  -> clamp and align to action space
  -> runtime policy validation
  -> result
```

Most online requests must not run an unbounded agentic loop. Future ephemeral runtime candidates require explicit governed permission covering the generator, action space, target authority, fallback, audit requirements, and latency budget.

## Experiment variant assignment

Variant assignment is the runtime execution mechanism for active experiment state. The experiment lifecycle is owned by the control plane.

```text
active experiment state
  + eligible assignment target
  -> deterministic bucket
  -> approved variant
  -> value and experiment metadata
```

Assignment should hash stable inputs such as:

```text
experimentId
  + allocationVersion
  + assignmentTargetKind
  + assignmentTargetId
  + salt
```

The same assignment target must receive the same variant across requests and service replicas for the same allocation version. Changing allocation semantics requires an explicit version change.

Long-lived experiments should normally use user, session, or tenant identity. Application instance identity should be used only for explicitly deployment-scoped experiments. Request-level assignment is appropriate only when reassignment between requests is intentional.

Runtime assignment must not:

- create or edit variants;
- change allocation weights;
- conclude or promote an experiment;
- bucket outside eligibility constraints;
- return values outside the definition action space.

## Rollout routing

Rollout routing applies the active rollout stage:

```text
active rollout state
  + eligible routing target
  -> deterministic inclusion
  -> previous or selected authority
```

Although rollout routing and experiment assignment may use similar hashing, their semantics differ. A rollout delivers a selected change progressively; an experiment creates comparable groups to measure alternatives.

## Runtime policy checks

Lifecycle governance approves durable authority. Runtime policy verifies that applying that authority to this request remains safe.

Runtime checks may include:

- definition and state compatibility;
- target eligibility;
- output type and bounds;
- required inference inputs;
- evidence freshness when evidence is required;
- cooldown, pause, or override when their contracts exist;
- experiment or rollout eligibility;
- conflict detection;
- fallback requirements.

Runtime policy cannot widen the approved state.

## RuntimeDecisionResult

A result should include:

- returned value;
- decision mode;
- fallback status and provenance;
- definition identity;
- governed state ID;
- runtime and control targets;
- strategy ID when applicable;
- policy result;
- confidence fields when applicable;
- decision and audit IDs;
- explanation summary.

Experiment assignment must additionally include:

```text
experimentId
variantId
allocationVersion
assignmentUnit
```

Rollout routing should similarly identify rollout ID, stage, and allocation version.

The result is a record of what this request received. It is not future authority.

## Decision records and exposure

Runtime execution creates a decision record before returning:

```text
RuntimeDecisionResult
  -> decisionId
  -> application applies or renders value
  -> confirmExposure(decisionId)
  -> exposureId
  -> attributed outcome telemetry
```

An exposure is recorded only after the client confirms that it applied or rendered the returned value. Experiment records must preserve experiment ID, variant ID, allocation version, and assignment unit so outcomes can be compared correctly.

## Fallback behavior

Fallback has explicit provenance:

- **Server fallback** is an audited runtime result produced when no permitted
  target has compatible active state, or applicable evidence or policy
  prevents normal execution. Missing-state fallback has no selected control
  target, state lineage, strategy identity, or exposure confirmation.
- **Client fallback** is permitted only for explicitly configured data-plane availability failures and cannot claim server decision, policy, audit, or exposure identity.
- **Contract/readiness errors**, including corrupt or incoherent persisted
  state, never become fallback.

Fallback remains inside the registered decision definition and effective policy.

## Example

```text
request:
  definition = tetris.dropInterval revision 2
  runtimeTarget = session:game-456
  inputs.tetris.boardPressure = 0.82
  inputs.tetris.recentPlacementTimeMs = 1420
  inputs.tetris.recoveryFailures = 2
  inputs.tetris.currentLevel = 8

resolved state:
  controlTarget = cohort:new_players
  mode = strategy
  threshold = 0.55
  valueAtOrAbove = 850
  valueBelow = 750

execution:
  weighted score = 0.6665
  score meets threshold
  output remains within bounds, step, and max delta from contract baseline 800

result:
  value = 850
  decisionMode = strategy
  fallbackUsed = false
```

## Design principles

1. **Execute authority; do not create it**.
2. **Deterministic by default** across retries and replicas.
3. **Bounded request path** with no unapproved agent loop.
4. **Exact contract identity** before execution.
5. **Explicit mechanism** for fixed, strategy, experiment, rollout, override, and fallback behavior.
6. **Runtime policy narrows only**.
7. **Exposure follows application** rather than merely recording a returned response.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
- [Decision Intelligence](DECISION_INTELLIGENCE.md)
- [Decision Lifecycles](DECISION_LIFECYCLES.md)
