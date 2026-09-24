# Runtime decision execution

## Purpose and invariants

Decision Service applies compatible approved state to one application request.
Execution is deterministic, bounded, constrained by the exact definition and
authority, explicit about fallback, and durably recorded before server success.
It cannot register definitions, generate proposals, approve candidates, repair
authority or mutate the active head.

## Request flow

```text
exact runtime identity + target/context + request inputs
  -> accepted Contract Store projection
  -> resolve ordered targets
  -> resolve all required request/evidence operands from one pinned generation
  -> read first compatible State Store authority
  -> active value, numeric rule or governed fallback
  -> deterministic decision constraints and exact output validation
  -> durable Evidence Store decision record
  -> runtime decision result
```

Current approved-definition receipts are not activation-ready receipts.
#40 adds that stronger registration guarantee after #49's server alignment.

Unknown, conflicting or retired identities are contract errors. Corrupt state
or unavailable required durable recording fails explicitly, never as a
success-shaped value.

## Target, input and state resolution

The definition permits target kinds and an explicit primary/fallback order.
Only those state targets are probed. A valid head for a different revision is
incompatible; malformed or incoherent state fails closed.

Context supplies target identifiers only through declared mappings. Cohort
claims require authoritative resolution. Evidence bindings may reference an
authorized target outside the selected state chain; each input records its
actual target, resolution source and original claim.

Request operands are type/range checked. Callers cannot override evidence
operands. Every evidence read pins authenticated tenant/app/environment, exact
definition, generation and evaluation time. There is no hot-path aggregation
or wait for telemetry export.

## Active value and numeric rule

An `active-value` returns its exact approved value after compatibility and
constraint evaluation. It has no strategy identity.

The numeric executor boundary is:

```text
runtime definition projection + approved numeric rule + resolved primitive map
  -> candidate or explicit execution error
```

It receives neither lifecycle state nor an `EvidenceSnapshot`, and never
queries telemetry. Weighted inputs use:

```text
normalized = clamp((value - minimum) / (maximum - minimum), 0, 1)
score = sum(normalized * weight) / sum(weight)
```

Weights are finite and nonnegative with a positive total. The exact selected
branch passes to constraint evaluation; runtime cannot clamp, step-align or
repair it into a third value. Deterministic rules return `confidence: null`,
not fabricated model uncertainty or evidence quality.

## Decision constraints

Constraints are definition data and Decision Service behavior, not a separate
Policy service. Checks include output type/bounds/step, fixed-default
max-delta, target/input eligibility, state compatibility and fallback. The
current implementation also supports explicitly declared cooldown, pause and
quality constraints through its existing internal evaluator.

`max-delta` compares to manifest `result.default`. With default `800` and delta
`50`, `750` and `850` are independently valid, including successive requests.
The existing cooldown guard uses governed last-change time, not the previous
returned value. The remaining Phase 3 constraint rebaseline is downstream.

Constraint evaluation may approve the exact candidate, require governed
fallback or reject execution. It cannot widen authority.

## Durable decision and exposure records

Before success, Decision Service persists authenticated scope, exact contract,
original caller inputs, resolved values and per-input provenance, targets,
selected strategy/mode, constraint results, fallback, returned value and time.
Source/target provenance survives evidence updates and exact decide replay.

The target architecture additionally carries complete state and activation
lineage. Current runtime responses do not expose `stateId`; #49/#40/#41 own
the executable record/authority alignment and final integrated verification.
Do not infer complete future lineage from today's approved-definition receipt.

```text
server decision -> application applies or renders value
  -> confirm with decisionId + confirmation token
  -> committed exposureId -> ordinary attributed outcome telemetry
```

Confirmation cannot replace decision-time inputs. It prepares a stable
identity, appends durable exposure evidence and commits confirmation, with
replay/recovery preserving one exposure. The current lookup remains in-memory;
an audit append alone is not completed confirmation.

Console/in-memory recording is limited to tests or explicitly non-ready debug
configurations. A ready append failure prevents server success. Explanation
is derived from stored facts, not a standalone service.

## Failure and fallback provenance

| Outcome | Meaning |
| --- | --- |
| Governed fallback | Valid registered request returned a durably recorded safe value. |
| SDK availability fallback | Explicit local response to an eligible outage, without server decision, constraint, record or exposure identity. |
| Contract error | Missing, unknown, conflicting or retired exact identity; not fallback-eligible. |
| Readiness error | Required contract, state or durable dependency is unavailable. |
| Invalid state | Corrupt/incoherent authority is never repaired into a value. |
| Required input evidence unavailable | Missing/stale/future/ambiguous/invalid operands produce 503 with SDK fallback forbidden. |

Missing-state fallback has no selected state/strategy identity. Request-only
decisions without evidence-dependent constraints access neither evidence
port. Separate quality requirements follow their explicitly declared failure
behavior; they do not manufacture input defaults.

## Future boundary

Experiments, rollout routing, overrides and learned strategies need explicit
state, lifecycle, constraint, record and bounded-execution contracts before
entering this request path. Async Analysis Pipeline is not invoked online.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision authority](AUTHORITY.md)
- [Decision Service](../design/decision-service/README.md)
- [State Store](../design/state-store/README.md)
- [Evidence Store](../design/evidence-store/README.md)
