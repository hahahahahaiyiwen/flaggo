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
- resolves required request/evidence operands from one pinned generation;
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
  -> required typed input resolution
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

Decision Service evaluates only the complete constraints declared by the
accepted runtime projection. Current checks include:

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
resolved primitive inputs, not lifecycle state or either evidence snapshot.
Weighted rules compute:

```text
score = sum(normalizedInput * weight) / sum(weight)
```

The selected branch is passed unchanged to constraint evaluation. Bundle-authored
rules report no learned confidence.

Missing, nonnumeric, nonfinite or duplicate rule inputs produce explicit
execution errors; the executor cannot default operands or choose fallback.
Normalization clamps each input to `[0,1]`, but the exact output branch is
never clamped or repaired. Total weight must be finite and positive.

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

The target record includes complete authority lineage; current implemented
records retain exact contract and strategy/target identity plus original
caller inputs, resolved values and per-input source/target provenance. Full
authority-lineage integration remains #49/#40/#41, not a property inferred
from current approved-definition receipts.

Exposure confirmation appends a separate exposure record only after the
application reports that it applied or rendered the value. Explanation text is
a deterministic projection of these structured facts, not a separate service.

If the durable append fails in a ready configuration, the request fails. A
console or volatile in-memory sink cannot authorize success.

## Implemented input, retry and confirmation behavior

The host supplies authenticated `ApplicationScope`, including tenant;
runtime JSON and OTel resource attributes cannot supply tenant authority.
Only declared request-owned primitive operands cross `inputs`. Duplicate
properties/old arrays, unknown fields, invalid primitives/bounds, missing
required context and evidence overrides are explicit failures.

`DecisionInputResolver` reads evidence through `IInputEvidenceReader` at one
generation/evaluation time, bound to scope, exact definition, binding and
resolved target. Unusable required inputs return 503
`required-evidence-unavailable` with SDK fallback forbidden. Request-only
decisions without quality constraints access neither evidence port.

State lookup uses declared primary/fallback targets only. Cohort claims need
authoritative verification/replacement. Evidence targets may differ from the
state fallback chain; their actual IDs, source and original claims are retained
in each evidence input's provenance and copied to exposure.

Retained decide replay uses the original result, inputs and provenance after
telemetry changes. Mutable evidence generations and correlation/tracing
metadata are not part of canonical caller identity. See
[retry semantics](../API_CONTRACT_PROPOSAL.md#correlation-and-retries).

Confirmation prepares a stable exposure, appends durable evidence and then
commits confirmation. Capabilities are omitted from record output. After the
append succeeds, bounded post-audit commit handles request cancellation and
shutdown without pretending an append alone is completed confirmation.
Exact replay preserves one exposure; conflicting observations fail.
The current state-library lookup is in-memory and accepts only completed
confirmation with exact tenant/app/environment/definition/target. New
references to lost confirmations fail after restart.

## APIs

Current data-plane APIs remain:

```http
POST /v1/decisions/{decisionKey}:decide
POST /v1/exposures/{decisionId}:confirm
```

Future query or operator experiences are clients of service APIs and durable
stores. They are not server components by themselves.

## Current implementation mapping

The data-plane host and `Flaggo.Decisioning`, `Flaggo.Policy` and `Flaggo.Audit`
libraries implement this behavior. Their current policy/audit wire names are
implementation details, not separate services. #49 owns executable ownership
and terminology alignment; #40 adds activation-converged registration.
Native OTLP routes in the same host belong to the separate
[OTel Ingestion](../otel-ingestion/README.md) boundary and ingest permission.

## Related documents

- [Runtime execution](../../architecture/RUNTIME_EXECUTION.md)
- [Contract Service](../contract-service/README.md)
- [State Store](../state-store/README.md)
- [Evidence Store](../evidence-store/README.md)
- [Shared contracts](../shared-contracts/README.md)
