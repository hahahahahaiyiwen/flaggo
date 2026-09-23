# Runtime decision execution

## Purpose

Decision Service applies compatible approved state to one application request.

It answers:

> Given the exact decision definition, runtime target, live inputs, active
> state, and decision constraints, what value should this request receive?

Runtime does not register definitions, generate proposals, approve candidates,
or mutate durable authority.

## Invariants

Runtime execution is:

- deterministic for the same identity, state, target, and inputs;
- bounded by the accepted definition, authority, and constraints;
- unable to widen or repair approved authority;
- explicit about fallback and failure provenance;
- durably recorded before server success; and
- unable to create authority.

## Request flow

```text
application request
  -> validate exact runtime identity
  -> require ready Contract Store projection
  -> resolve ordered exact targets
  -> read compatible active State Store authority
  -> execute active value or numeric rule
  -> evaluate decision constraints
  -> select exact result or governed fallback
  -> append durable Evidence Store decision record
  -> return RuntimeDecisionResult
```

Missing, conflicting, or retired identity is a contract error. Corrupt state,
non-ready registration, or unavailable required durable recording is a
readiness/integrity error. None silently becomes a value.

## Target and state resolution

The definition declares one primary runtime target plus explicit
`fallbackOrder`. Decision Service resolves only those exact target roles and
loads the first active state matching the complete runtime identity.

A valid head for another revision is incompatible and resolution may continue.
Malformed or incoherent state fails closed.

## Active value

`active-value` authority returns its exact approved value after compatibility
and constraint evaluation. It has no strategy identity.

## Numeric rule

`numeric-rule` authority consumes the runtime projection, approved rule, and
typed live inputs:

```text
normalized = clamp((value - minimum) / (maximum - minimum), 0, 1)
score = sum(normalized * weight) / sum(weight)
```

Weights are finite and non-negative with a positive total. Threshold is within
`[0, 1]`. The exact selected branch proceeds to constraint evaluation; runtime
does not clamp or step-align it.

Bundle-authored rules return `confidence: null`.

## Decision constraints

Current runtime constraints include:

- output type, numeric bounds, allowed values, and step;
- fixed-default `max-delta`;
- required live inputs and target eligibility;
- exact state/definition compatibility; and
- fallback requirements.

For Phase 3, `max-delta` compares every candidate with
`actionSpace.default`. With default `800` and delta `50`, both `750` and `850`
are valid independently.

Constraint evaluation may approve the exact candidate, require governed
fallback, or reject execution. It cannot widen authority.

## Governed fallback

A valid ready definition may return its registered fallback when no permitted
target has compatible active state or an applicable constraint requires it.

Missing-state fallback has no selected state or strategy identity. Fallback is
a runtime outcome, not a persisted authority kind.

## Durable decision and exposure records

Before success, Decision Service appends a decision record containing:

- exact definition identity;
- runtime and selected control targets;
- live inputs;
- state, activation, and approval lineage;
- candidate, applied constraints, and fallback provenance;
- returned value and timestamp.

The application confirms exposure only after using the value. Confirmation is
idempotent and appends a separate exposure record. Later outcomes correlate to
`exposureId`.

Explanation text is derived from these stored facts.

## Failure provenance

| Outcome | Meaning |
| --- | --- |
| Governed server fallback | Ready server request returned an audited safe fallback. |
| SDK availability fallback | Client-local response to a recognized outage; no server decision or exposure identity. |
| Contract error | Missing, conflicting, unknown, or retired exact identity. |
| Readiness error | Required contract, state, or durable store is unavailable or non-ready. |
| Invalid state | Persisted authority is corrupt or incompatible and is never repaired. |

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Decision authority](AUTHORITY.md)
- [Decision Service](../design/decision-service/README.md)
- [State Store](../design/state-store/README.md)
- [Evidence Store](../design/evidence-store/README.md)
