# Decision Service design

## Purpose

Decision Service is the online data-plane service applications call to receive
one bounded runtime value and to confirm that a returned value was applied.

The HTTP API is an interface of this service, not a separate architectural
component.

## Responsibilities

Decision Service:

- validates application identity and complete expected runtime identity;
- loads the immutable runtime definition projection from Contract Store;
- rejects unknown, conflicting, retired, or non-ready definitions;
- resolves the exact target chain declared by the definition;
- reads the first compatible active authority from State Store;
- evaluates active values or bounded numeric rules using live runtime inputs;
- evaluates applicable decision constraints without widening authority;
- selects governed fallback when the registered contract permits it;
- appends a durable decision record before returning server success;
- confirms exposure idempotently and appends the exposure record; and
- returns compact decision and fallback provenance.

It cannot register a definition, approve a candidate, mutate active authority,
or query raw telemetry on the request path.

## Runtime flow

```text
exact identity + runtime target + live inputs
  -> ready runtime projection
  -> ordered target resolution
  -> compatible active state
  -> active-value or numeric-rule execution
  -> decision-constraint evaluation
  -> governed result or fallback
  -> durable decision record
  -> RuntimeDecisionResult
```

The request path is deterministic and bounded. An unbounded agent or async
analysis loop never runs inside it.

## Decision constraints

Decision Service evaluates only constraints declared by the accepted runtime
projection or a separately approved narrowing layer. Current checks include:

- value type, bounds, allowed values, and numeric step;
- fixed-default `max-delta`;
- target and required-input eligibility;
- candidate/state compatibility; and
- fallback availability.

A constraint may approve the exact candidate, require governed fallback, or
reject execution. It cannot repair, clamp, step-align, or synthesize a value
outside approved authority.

## Runtime execution capabilities

`active-value` returns its exact approved value after constraint evaluation.

`numeric-rule` consumes only the runtime projection, approved rule, and typed
live inputs. Weighted rules compute:

```text
score = sum(normalizedInput * weight) / sum(weight)
```

The selected branch is passed unchanged to constraint evaluation. Bundle-authored
rules report no learned confidence.

These executors are internal Decision Service capabilities, not a separate
reasoning service.

## Durable records and explanation

Before server success, Decision Service appends an immutable decision record to
Evidence Store. The record contains enough facts to reconstruct the result:

- exact definition identity and targets;
- runtime inputs;
- selected authority and activation lineage;
- candidate, applied constraints, and fallback provenance;
- returned value, decision identity, and timestamp.

Exposure confirmation appends a separate exposure record only after the
application reports that it applied or rendered the value. Explanation text is
a deterministic projection of these structured facts, not a separate service.

If the durable append fails in a ready configuration, the request fails. A
console or volatile in-memory sink cannot authorize success.

## APIs

Current data-plane APIs remain:

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

Future query or operator experiences are clients of service APIs and durable
stores. They are not server components by themselves.

## Current implementation mapping

The current data-plane host and `Flaggo.Decisioning`, `Flaggo.Policy`, and
`Flaggo.Audit` libraries implement parts of this service. Issue #40 owns the
executable ownership migration. This document does not require an immediate
assembly rename.

## Related documents

- [Runtime execution](../../architecture/RUNTIME_EXECUTION.md)
- [Contract Service](../contract-service/README.md)
- [State Store](../state-store/README.md)
- [Evidence Store](../evidence-store/README.md)
- [Shared contracts](../shared-contracts/README.md)
