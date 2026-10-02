# Runtime evaluation model

## Status

This document defines the conceptual model for evaluating one accepted
`DecisionContract` at runtime. Its wire shape is implemented by
`contracts/schemas/runtime-models-v3.schema.json` and
`contracts/openapi/flaggo-runtime-v3.yaml`.

Contract acceptance, executable generation, and activation are
defined in [Decision contract lifecycle](LIFECYCLE.md). Contract and executable
shapes are defined in
[Decision contracts and executables](CONTRACTS.md).

## Purpose

The SDK constructs a complete `RuntimeInput`. The runtime server resolves the
active immutable executable for one exact contract version, evaluates it, and
returns the result without retaining per-request state:

```text
application attributes
  -> SDK binding
  -> SDK internal attributes + current exposure context
  -> complete RuntimeInput

authenticated application/environment scope
  + contractDigest
  + complete RuntimeInput
  -> find ActiveExecutable
  -> validate input
  -> deterministic bounded evaluation
  -> validate result
  -> return RuntimeDecision
```

Runtime Evaluation never accepts contracts, selects a current contract version
by name, generates executables, analyzes evidence, changes activation, or
stores an evaluation session.

## Identity and authority

The authenticated principal establishes application and environment scope.
Contract attributes and exposure context never establish that scope and are
never authentication or authorization data.

Runtime authority is:

```text
RuntimeActivation[
  authenticated application/environment scope,
  contractDigest
] = executableDigest
```

The route carries both the decision contract name and exact `contractDigest`.
The name identifies the logical resource and the digest identifies one
immutable version belonging to it. Runtime validates that relationship but
never resolves the named resource's current version.

The resolved executable must:

- exist and be active for the requested digest;
- contain that same `contractDigest`;
- have an `executableDigest` matching its canonical content; and
- satisfy the accepted contract.

An unknown contract digest, missing activation, or inconsistent contract,
activation, and executable identity fails explicitly. Runtime never substitutes
another digest or reconstructs an executable from `result.default`.

## Runtime evaluation request

The runtime operation and request body are:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
Content-Type: application/json
```

```yaml
attributes:
  session_id: game-456
  board_pressure_mean_5s: 0.82
  board_pressure_max_5s: 0.91
  current_level: 10
  _random: 0.137...

currentExposure:
  exposureId: exp_...
```

The route selects runtime authority. The request body is the complete
`RuntimeInput`, constructed before the request reaches the server:

- `attributes` contains application-bound and SDK-generated internal values;
- optional `currentExposure` identifies the previous decision that the
  application actually applied; and
- application and environment come from authentication rather than the body.

`currentExposure` is absent when no previous decision has been applied in the
SDK's current activity context. It is exposure context, not proof that the new
result will be used, and it does not select the contract or executable.

The initial v3 wire shape carries only `currentExposure.exposureId`. How that
identity is allocated and whether a future version needs more correlation
fields remain open. The runtime server does not retain a session.

## Complete RuntimeInput

The SDK, not the runtime server, constructs the complete `RuntimeInput`:

```text
application-bound attributes
  + SDK-generated internal attributes
  + optional current exposure
  -> RuntimeInput
```

Application code may bind any schema-valid subset of declared attributes. The
SDK:

- rejects application attempts to bind names beginning with `_`;
- validates or pre-validates application values where the contract is locally
  available;
- generates the internal attributes required by the contract-semantics
  version;
- begins with `_random`, a number in `[0, 1)`; and
- reuses the same internal values when retrying one logical evaluation.

The runtime server validates the supplied complete input but does not add,
replace, or persist internal attributes. A client that bypasses the SDK must
follow the same reserved-name and internal-attribute protocol.

For user-declared attributes:

- an undeclared name is rejected;
- an invalid supplied value is rejected;
- a missing value is permitted; and
- a rule depending on a missing value is ineligible.

For internal attributes:

- required internal values must be present;
- their names and values must satisfy the contract-semantics version; and
- application code cannot override the SDK-generated values.

The bounded JSON values permitted in `attributes` follow the contract's JSON
Schema profile. The runtime wire model must not introduce an independent,
narrower type system.

`_random` is generated once for one logical SDK evaluation. Transport retries
reuse that value. Different logical evaluations may use different values, so
determinism is:

```text
evaluate(DecisionExecutable, complete RuntimeInput) -> result
```

It is not identical results for application attributes alone.

## Stateless server evaluation

The runtime server handles each request independently:

1. Authenticate application and environment scope.
2. Validate `contractDigest` and the complete `RuntimeInput`.
3. Read the active `executableDigest` for that exact digest and scope.
4. Load the immutable `DecisionExecutable`.
5. Evaluate the executable.
6. Validate the result against the accepted contract.
7. Return the `RuntimeDecision`.

The server does not persist an evaluation snapshot, decision session, caller
attributes, internal attributes, or response as part of Runtime Evaluation.

Resolving an active executable does not require durable per-request pinning.
The request simply evaluates the immutable executable returned by its lookup.
If activation changes afterward, a later lookup may return the replacement;
the in-progress request continues using the immutable object it already
obtained. No rules or metadata from two executables are combined.

The request states are:

```text
Received
  -> Validated
  -> Active Executable Resolved
  -> Evaluated
  -> Result Validated
  -> Responded
```

Any failure ends the request with an explicit error. No state from the failed
request is required by a later request.

## Rule evaluation

The initial executable kind is `rules`:

1. Consider rules in their declared order.
2. A rule is eligible only when every user or internal attribute referenced by
   its predicate and return expression is present.
3. Skip an ineligible rule.
4. Evaluate an eligible `when.expression`.
5. The first expression returning `true` selects that rule.
6. Evaluate its literal or expression return.
7. If no rule is selected, use `DecisionContract.result.default`.
8. Validate the selected result against `DecisionContract.result.schema`.

No-match default behavior is a successful evaluation, not a fallback or error.
It does not indicate degraded service and does not authorize the runtime to
hide invalid input, expression failure, corrupt executable content, or missing
authority.

CEL source is parsed, type-checked, and compiled before activation. Runtime
evaluates only the checked bounded representation. A runtime evaluator failure
is explicit even if the contract has a valid default.

`currentExposure` is contextual information about a previous applied decision.
It does not influence rule evaluation unless a future contract-semantics
version explicitly exposes some part of it as a declared internal attribute.

## RuntimeDecision

The conceptual successful response is:

```yaml
contractDigest: sha256:...
executableDigest: sha256:...

result: 250

evaluation:
  source: rule
  rule: high-pressure
```

For no-match behavior:

```yaml
contractDigest: sha256:...
executableDigest: sha256:...

result: 500

evaluation:
  source: default
```

The response exposes the identities required to identify which contract and
executable produced the result. It does not need definition IDs, server
revisions, bundle digests, compatibility classifications, target-resolution
chains, strategy types, fabricated confidence, or a generic constraint report.

The initial v3 response exposes the selected rule name for rule evaluations.
Whether to expose evaluation time or a decision/exposure correlation identity
in a future version remains open. The stateless runtime does not require those
fields to evaluate a request.

## SDK decision observation flow

A returned `RuntimeDecision` proves only that Flaggo evaluated a result. It does
not prove that the application used it.

```text
RuntimeDecision
  -> SDK emits flaggo.decision.received through OpenTelemetry
  -> application may apply or render result
  -> next RuntimeInput may carry a previous applied exposure as currentExposure
```

The built-in observation records receipt, not application. The runtime server
does not maintain an exposure session. Receiving `currentExposure` lets a
request carry previous applied-decision context without requiring affinity to
an earlier server instance.

The exact current-exposure fields and retry behavior remain deferred.

## Retry behavior

The runtime server does not persist idempotency records. The SDK reuses the
same complete `RuntimeInput`, including `_random`, when retrying one logical
evaluation.

If the same immutable executable remains active, deterministic evaluation
returns the same result. If activation changes between attempts, a stateless
retry resolves and evaluates the replacement executable. This is expected:
retry preserves the logical input, not the previously active executable.
The request does not carry an expected `executableDigest` or evaluation token
that prevents normal activation changes from taking effect.

## Failure and fallback model

| Outcome | Runtime behavior |
| --- | --- |
| Unknown `contractName`, unknown `contractDigest`, or name/digest mismatch | Explicit not-found contract-version error. |
| Invalid application or internal attribute | Explicit request-validation error. |
| Missing user attribute | Permitted; rules that depend on it are ineligible. |
| Missing required internal attribute | Explicit invalid-input error. |
| Missing activation or corrupt identity chain | Explicit authority/readiness error; never reconstruct or substitute. |
| No eligible matching rule | Successful contract-default result. |
| Expression or evaluator failure | Explicit evaluation error; never return the default. |
| Result violates `result.schema` | Explicit executable/evaluation integrity error. |
| Service unavailable | Explicit server error; any SDK-local fallback occurs outside this runtime contract. |

The initial HTTP failure mapping is:

| Problem type suffix | Status | Use |
| --- | --- | --- |
| `invalid-request` | 400 | Malformed JSON, invalid path/header syntax, or structurally invalid wire input |
| `authentication-required` | 401 | Missing or invalid bearer credentials |
| `insufficient-scope` | 403 | Missing `flaggo.decisions:decide` |
| `contract-version-not-found` | 404 | Unknown name/digest or a digest not owned by the route name in the authenticated scope |
| `unsupported-media-type` | 415 | Request is not `application/json` |
| `invalid-runtime-input` | 422 | Complete input violates the accepted contract or internal-attribute profile |
| `rate-limited` | 429 | Runtime admission limit; includes `Retry-After` |
| `executable-not-active` | 503 | No executable is active for the exact scope and digest; includes `Retry-After` |
| `dependency-unavailable` | 503 | A required Contract or Executable Store read is unavailable; includes `Retry-After` |
| `executable-integrity-failure` | 500 | Stored executable identity or checked representation is inconsistent |
| `evaluation-failed` | 500 | Checked expression evaluation fails or exhausts its runtime budget |
| `invalid-runtime-result` | 500 | Evaluated result violates the accepted result schema |
| `internal-error` | 500 | Unclassified server failure |

Each suffix is rooted at `https://flaggo.dev/problems/`. Responses contain only
RFC 9457 named members. Correlation and retry metadata use
`X-Flaggo-Correlation-Id` and `Retry-After` headers.

An SDK may later support fallback to the contract default or the last evaluated
decision. Selection, retention, expiry, provenance, and exposure behavior for
those SDK-local results are deferred. Server no-match default behavior remains
a successful `RuntimeDecision` and must not be conflated with a client fallback.

## Invariants

1. Authentication establishes application and environment scope.
2. `contractName` identifies the logical DecisionContract resource.
3. `contractDigest` selects one exact accepted version belonging to that name.
4. Runtime never resolves the management resource's current version.
5. The SDK constructs the complete `RuntimeInput`.
6. Complete input includes required internal attributes and may include the
   previous applied exposure.
7. The runtime server does not generate or persist request attributes.
8. Each request resolves one immutable active executable and evaluates it
   without server-side session state.
9. Missing user attributes affect rule eligibility; invalid attributes are
   errors.
10. Evaluation is deterministic for one executable and complete runtime input.
11. No-match returns the contract default as a normal result.
12. Failures are never disguised as contract-default results.
13. Returning a decision does not imply exposure.

## Open design questions

1. How is `currentExposure.exposureId` allocated, and will future versions need
   more correlation fields?
2. Which additional diagnostic and correlation fields belong in a future
   `RuntimeDecision`?
3. What are the final generation, distribution, and future stable-bucketing
   semantics for SDK-generated `_random`?
4. What policy governs future SDK fallback to the contract default or last
   evaluated decision?

## Related documents

- [Decision contracts and executables](CONTRACTS.md)
- [Decision contract lifecycle](LIFECYCLE.md)
- [Runtime client and Decision Service](../architecture/RUNTIME.md)
