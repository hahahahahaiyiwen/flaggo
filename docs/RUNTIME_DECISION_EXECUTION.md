# Runtime Decision Execution

## Purpose

Runtime decision execution is the data-plane capability that applies compatible approved state to one application request.

It answers:

> Given this decision definition, runtime target, request context, policy, and active governed state, what value should this request receive?

Runtime execution does not analyze which candidate is globally best or mutate
lifecycle authority. Experiment, rollout, and override execution remain future
mechanisms until their state, lifecycle, policy, and result contracts are
separately approved.

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

Runtime execution is source-independent by design. Phase 3 receives authority
from an approved bundle candidate. A future proposal-managed source must
produce the same current governed-state shape or introduce a separately
approved extension, and must pass the same definition, target, output, and
runtime-policy checks.

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
  -> resolve active-value authority, evaluate numeric-rule authority,
     or produce governed fallback
  -> validate returned value
  -> record decision inputs, state/policy lineage, and applicable evidence
  -> RuntimeDecisionResult
```

Missing, unknown, conflicting, or retired definition identity is a contract error. It must not silently become fallback.

## Execution mechanisms

| Mechanism | Runtime responsibility |
| --- | --- |
| Active-value resolution | Return the approved active value. |
| Numeric-rule evaluation | Evaluate the approved deterministic numeric rule against declared inputs. |
| Fallback | Return the registered safe value when no permitted target has compatible active state, or applicable evidence/runtime policy prevents normal execution. |

The mechanism is selected from governed state; the runtime does not choose a
new lifecycle. Experiment assignment, rollout routing, and override require
future contract extensions and are not current execution mechanisms.

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

## Active-value resolution

Active-value state returns one approved value after compatibility and policy
checks:

```text
active fixed value
  -> validate output type, range, and allowed values
  -> return value
```

## Numeric-rule evaluation

A Phase 3 numeric rule is the approved bounded strategy for request-time
evaluation. It computes the declared weighted score, selects the declared
threshold branch, and returns one of the two approved numeric values.

```text
active numeric-rule strategy
  + declared inference inputs
  -> compute weighted score
  -> select threshold branch
  -> clamp and align to action space
  -> runtime policy validation
  -> result
```

Future strategy kinds or ephemeral runtime candidates require explicit
governed contracts covering the evaluator, action space, target authority,
fallback, audit requirements, and latency budget. Most online requests must
not run an unbounded agentic loop.

## Future experiment variant assignment

This section is conceptual future behavior, not a current runtime contract.
Variant assignment would be the runtime execution mechanism for approved
experiment state; the experiment lifecycle would remain in the control plane.

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

## Future rollout routing

This section is conceptual future behavior, not a current runtime contract.
Rollout routing would apply an approved active rollout stage:

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
- conflict detection;
- fallback requirements.

Runtime policy cannot widen the approved state.
Cooldown, pause, override, experiment eligibility, and rollout eligibility are
future checks that require separately approved contracts.

## RuntimeDecisionResult

A result should include:

- returned value;
- decision mode;
- fallback status and provenance;
- definition identity;
- runtime and control targets;
- strategy ID when applicable;
- policy result;
- confidence (`null` for Phase 3 bundle-approved authority);
- decision and audit IDs;
- explanation summary.

Governed state identity and activation lineage remain in
`AuditRecord.stateSummary`; the Phase 3 runtime response does not expose a
`stateId`.

Future experiment assignment results would additionally require:

```text
experimentId
variantId
allocationVersion
assignmentUnit
```

Future rollout results would similarly identify rollout ID, stage, and
allocation version. These fields are not part of the current Phase 3 result.

The result is a record of what this request received. It is not future authority.

## Decision records and exposure

Runtime execution creates a decision record before returning:

```text
RuntimeDecisionResult
  -> decisionId
  -> application applies or renders value
  -> narrow to ServerDecisionReceipt
  -> require exposure.confirmationRequired
  -> confirmExposure(decisionId, exposure.confirmToken)
  -> exposureId
  -> attributed outcome telemetry
```

An exposure is recorded only after the client confirms that it applied or
rendered the returned value. A future experiment contract must define the
experiment ID, variant ID, allocation version, and assignment unit preserved
for outcome comparison.

## Fallback behavior

Fallback has explicit provenance:

- **Server fallback** is an audited runtime result produced when no permitted
  target has compatible active state, or applicable evidence or policy
  prevents normal execution. Missing-state fallback has no selected control
  target, state lineage, strategy identity, or exposure confirmation.
- **Client fallback** is permitted only for explicitly configured data-plane availability failures and cannot claim server decision, policy, audit, or exposure identity.
- **Contract/readiness errors**, including corrupt or incoherent persisted
  state, never become fallback. Pending required activation returns
  `definition-not-ready`; failed persistence readiness returns
  `decision-service-not-ready`; invalid persisted authority returns
  `invalid-decision-state`. None is client-fallback eligible.

Fallback remains inside the registered decision definition and effective policy.

## Example

```text
request:
  decisionKey = tetris.dropInterval
  expectedContract.definitionId = def_01JQ8Y7M6X3K9P2W4R5T6V7N8A
  expectedContract.revision = rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3
  expectedContract.contractDigest = sha256:contract...
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
  valueType = number
  decisionMode = strategy
  strategyId = strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5
  confidence = null
  fallback.source = server
  fallback.resolutionFallbackUsed = false
  fallback.decisionFallbackUsed = false
  fallback.reason = null
```

## Design principles

1. **Execute authority; do not create it**.
2. **Deterministic by default** across retries and replicas.
3. **Bounded request path** with no unapproved agent loop.
4. **Exact contract identity** before execution.
5. **Explicit current mechanisms** for active-value, numeric-rule, and governed fallback behavior.
6. **Runtime policy narrows only**.
7. **Exposure follows application** rather than merely recording a returned response.

## Related documents

- [Mental Model](MENTAL_MODEL.md)
- [Decision Definition](DECISION_DEFINITION.md)
- [Decision Evidence](DECISION_EVIDENCE.md)
- [Decision Intelligence](DECISION_INTELLIGENCE.md)
- [Decision Lifecycles](DECISION_LIFECYCLES.md)
