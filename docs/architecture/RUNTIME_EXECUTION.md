# Runtime decision execution

## Purpose

Runtime decision execution is the data-plane capability that applies compatible
approved state to one application request.

It answers:

> Given the exact decision definition, runtime target, live inputs, compatible
> governed state, and runtime policy, what value should this request receive?

Runtime does not generate proposals, approve candidates, or mutate durable
authority.

## Runtime invariants

Runtime execution must be:

- deterministic for the same exact identity, state, target, and inputs;
- bounded by the definition, authority, and policy;
- fast enough for the application request path;
- consistent across service replicas;
- explicit about fallback and failure provenance;
- durably audited before a successful response;
- unable to create authority.

An unbounded request-time agent loop is prohibited. Any future request-time
mechanism requires a separately approved bounded contract with enforceable
latency and resource budgets.

## Request flow

```text
application request
  -> validate { definitionId, revision, contractDigest }
  -> validate registration and dependency readiness
  -> resolve ordered exact targets
  -> select the first compatible governed state
  -> resolve active value, evaluate numeric rule, or select governed fallback
  -> apply runtime policy
  -> validate the exact output
  -> durably persist audit
  -> return RuntimeDecisionResult
```

Missing, unknown, conflicting, or retired identity is a contract error.
Corrupt state, torn persistence, non-ready registration, and unavailable
required durable audit are readiness or integrity errors. None silently becomes
a decision value.

## Target and state resolution

The definition declares a primary inference target and explicit
`fallbackOrder`:

```text
runtime target + verified context + fallbackOrder
  -> ordered exact resolution targets
  -> stable authority head for each target
  -> first active state matching the complete runtime identity
```

The target hierarchy authorizes target kinds but does not insert undeclared
levels. A well-formed state for another revision is incompatible and resolution
may continue. Corrupt or incoherent state fails explicitly.

Audit distinguishes the runtime target receiving the result, the control target
owning state, any evidence target used by policy, and the fallback target.

## Current execution mechanisms

### Active value

`active-value` authority returns its approved value after compatibility and
runtime-policy validation. It has no strategy identity.

```text
approved value
  -> validate type, range, allowed values, step, and policy
  -> return exact value or governed fallback
```

### Numeric rule

`numeric-rule` authority evaluates declared live `SignalInput[]` through the
approved deterministic rule. The executor boundary is exactly:

```text
RuntimeDefinitionProjection
  + NumericRuleStrategy
  + SignalInput[]
  -> numeric candidate or execution error
```

`EvidenceSnapshot` does not cross this executor boundary. Evidence remains
available to policy, audit/explanation, and future proposal producers.

For each weighted input:

```text
normalized = clamp((value - minimum) / (maximum - minimum), 0, 1)
```

The score is the normalized weighted average:

```text
score = sum(normalized * weight) / sum(weight)
```

Weights must be finite and non-negative, with a finite positive total; they do
not need to sum to `1`. The threshold must be finite and within `[0, 1]`.

```text
score >= threshold -> valueAtOrAbove
score < threshold  -> valueBelow
```

Both branches are validated during bundle and activation processing. Runtime
passes the exact selected branch to policy. It never clamps, step-aligns, or
repairs an invalid persisted branch into a third value.

Bundle-authored rules report `confidence: null`; they do not fabricate model
confidence or evidence quality.

### Governed fallback

A valid registered definition may return its audited server fallback when no
permitted target has compatible active state or runtime policy prevents normal
execution.

Missing-state fallback has no selected control target, state lineage, strategy
identity, or exposure confirmation. Fallback is a runtime outcome, not a
persisted authority kind.

## Runtime policy

Runtime policy verifies that applying approved authority to this request remains
safe. Current checks include:

- exact definition and state compatibility;
- target eligibility;
- required live inputs;
- output type, bounds, allowed values, and step;
- fixed-baseline `max-delta`;
- conditional evidence requirements when a policy explicitly requires them;
- fallback requirements.

For Phase 3, `max-delta` compares every candidate with the fixed
`actionSpace.default`. With default `800` and delta `50`, both `750` and `850`
are valid independently, including a `750 -> 850` request sequence. Cooldown,
hysteresis, and previous-result stabilization remain separate future work.

Policy may approve the exact candidate, return governed fallback, or reject it.
Policy cannot widen authority or synthesize a repaired candidate.

## Runtime result

A successful `RuntimeDecisionResult` includes:

- the returned primitive value;
- decision mode;
- fallback status and provenance;
- complete definition identity;
- runtime and selected control targets;
- `strategyId` only for numeric-rule authority;
- policy result;
- confidence (`null` for bundle-authored Phase 3 authority);
- decision and audit IDs;
- explanation summary.

Governed state identity and activation lineage remain in the audit state
summary; the current runtime response does not expose `stateId`.

The result records what this request received. It is not future authority.

## Durable audit and exposure

A ready service must commit the decision audit to storage that survives process
failure before returning success. Console and in-memory sinks are limited to
tests or explicitly non-ready debugging.

The audit captures the exact runtime identity, request inputs, targets, selected
state and activation lineage, authority kind, exact candidate, policy outcome,
fallback status, result, and timestamp.

A returned result is not proof that the application used it:

```text
RuntimeDecisionResult
  -> application applies or renders value
  -> narrow to ServerDecisionReceipt
  -> confirm exposure with decisionId + confirm token
  -> exposureId
  -> outcome telemetry linked to exposureId
```

Confirmation is idempotent and cannot replace decision-time inputs. Raw domain
telemetry remains unlinked when it was not caused by an applied decision.

## Fallback and failure provenance

| Outcome | Meaning |
| --- | --- |
| Governed server fallback | Valid registered request produced an audited safe value. |
| SDK availability fallback | Explicitly configured response to a recognized data-plane outage; has no server decision, policy, audit, or exposure identity. |
| Contract error | Missing, unknown, conflicting, or retired exact identity; never fallback-eligible. |
| Readiness error | Required activation, state, audit, or persistence dependency is not ready; never a decision value. |
| Invalid decision state | Persisted authority is corrupt or incompatible; never repaired or converted to fallback. |

## Future extension boundary

Experiment assignment, rollout routing, override, and evidence-backed runtime
strategies are not current mechanisms. Each requires explicit state, lifecycle,
policy, result, audit, and bounded-executor contracts before it can enter the
request path.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Authority](AUTHORITY.md)
- [Decision definition](DECISION_DEFINITION.md)
- [Evidence](EVIDENCE.md)
- [Tetris scenario](../scenarios/TETRIS.md)
- [Decision API component](../design/decision-api/README.md)
- [Reasoning engine component](../design/reasoning-engine/README.md)
- [Policy component](../design/policy/README.md)
