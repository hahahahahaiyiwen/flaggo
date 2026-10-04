# Runtime client and Decision Service

## Purpose

This document describes the runtime path from application code through the
Flaggo SDK and Decision Service to an applied result and exposure observation.
It defines client and service responsibilities, online dependencies,
statelessness, concurrency, and failure boundaries.

It does not redefine `RuntimeInput`, `RuntimeDecision`, rule evaluation, or
fallback semantics. Those belong to
[Runtime evaluation model](../contracts/RUNTIME_EVALUATION.md), and the Runtime
API v3 OpenAPI document and schema are the wire-level authority.

## System context

```text
application
  -> application attribute bindings
  -> Flaggo SDK
       -> complete RuntimeInput
       -> exact-version Runtime API request
  -> Decision Service
       -> accepted Contract Store
       -> Executable Store active lookup
       -> bounded evaluator
  -> RuntimeDecision
  -> SDK emits flaggo.decision.received through OpenTelemetry
  -> application applies result
```

The SDK and Decision Service form one runtime integration boundary, but they
have different responsibilities. The SDK constructs complete input and records
that a decision response was received. The service resolves authority and
evaluates one immutable executable.

## Application and SDK responsibilities

Application build or deployment configuration supplies the exact
`{ contractName, contractDigest }` accepted through the management path. The
runtime SDK does not discover the current contract version.

For one logical evaluation, application code:

1. provides any available values for user-declared contract attributes;
2. asks the SDK to evaluate the exact contract version;
3. receives either a `RuntimeDecision` or an explicit failure;
4. decides whether to apply the returned result; and
5. reports the exposure through the SDK after applying the result.

The SDK:

- rejects application attempts to bind reserved internal names;
- constructs the complete `RuntimeInput`;
- adds contract-version-defined internal attributes, initially `_random`;
- reuses the same complete input for transport retries of one logical
  evaluation;
- carries the previous applied exposure when available;
- authenticates and sends the exact-version request;
- does not reinterpret server failures as successful decisions; and
- emits a raw decision-received observation after a successful decision when
  SDK telemetry is configured.

The current TypeScript SDK phase implements this flow through the returned
`RuntimeDecision`. The decision-received observation is not proof that the
application applied the result. Application outcome telemetry and async
analysis own later interpretation and correlation.

Future SDK-local fallback to a contract default or cached prior decision is a
separate policy. It does not change the Decision Service response or create a
server decision retroactively.

## Runtime API boundary

The Decision Service exposes:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

The route identifies the logical decision and one exact immutable contract
version. The body carries only the complete `RuntimeInput`; authenticated
credentials establish tenant, application, and environment scope.

Contract attributes, user identifiers, and exposure context are decision data.
They cannot authenticate a caller, authorize another scope, or select a
different contract version.

## Request handling

Each request executes one independent sequence:

```text
authenticate and authorize scope
  -> verify contractName owns contractDigest
  -> load accepted contract version
  -> validate complete RuntimeInput
  -> IExecutableStore.GetActive(scope, contractDigest)
  -> evaluate bounded behavior
  -> validate result against DecisionContract
  -> return RuntimeDecision
```

The service captures one immutable executable reference for the request after
activation resolution. An activation replacement after that lookup does not
alter the in-progress evaluation.

Detailed rule eligibility, no-match default behavior, response provenance,
and error categories remain defined in the runtime contract document rather
than repeated here.

## Decision Service responsibilities

The Decision Service owns:

- authentication and authorization for runtime requests;
- route name/digest relationship validation;
- complete input validation against the accepted contract;
- active executable resolution for the authenticated scope and exact digest;
- immutable executable integrity checks;
- bounded deterministic evaluation;
- result validation;
- Runtime API response and Problem Details mapping; and
- operational health, metrics, logs, and traces.

It does not own:

- contract deployment or current-version selection;
- executable generation or activation;
- application attribute binding;
- evidence queries or learning analysis;
- SDK-local fallback policy;
- confirmation or exposure sessions; or
- durable persistence of individual runtime requests or responses.

## Online dependencies

| Dependency | Request-path use |
| --- | --- |
| Accepted Contract Store | Verify name/digest identity and validate input and result |
| Executable Store | Atomically resolve the immutable executable whose scoped lifecycle state is Active for the exact digest |
| Evaluator | Execute the already checked representation |

Evidence storage, learning scheduling, executable generation, management
current-version lookup, and OpenTelemetry outcome materialization are not
online dependencies.

Accepted contracts and executable artifacts are immutable and may be cached by
digest. Executable lifecycle state is mutable and must be resolved with
semantics equivalent to one authoritative `GetActive` lookup per request. A
cache is acceptable only if its coherence contract preserves activation
replacement behavior.

## Statelessness and scaling

The semantic request path persists no evaluation session, caller attributes,
internal attributes, idempotency record, response record, or exposure state.
Any Decision Service replica can process a request when it can reach the
required authority data.

Operational telemetry may record request facts for service observability. Such
telemetry does not become ambient evaluator input and does not make a runtime
response a durable domain record.

Readiness verifies that required stores expose their exact supported schema
version and readable owned tables. It does not scan every stored artifact.
Exact contract and executable integrity is enforced when that authority is
resolved, before evaluation. Optional observability exporters may degrade
without changing decision semantics. The Runtime API defines the externally
visible health responses.

## Activation concurrency and retries

The Decision Service reads one activation and evaluates the referenced
immutable executable:

```text
attempt A: RuntimeActivation[scope, D] = E1 -> evaluate E1
activation replacement:                    E1 -> E2
attempt B: RuntimeActivation[scope, D] = E2 -> evaluate E2
```

Attempt A completes with `E1` even if replacement occurs during evaluation.
A later request or retry may use `E2`. This is expected; the SDK preserves the
complete logical input for retry but does not pin the prior executable.

Runtime never scans candidate generation history or chooses the latest
`createdAt`. Automatic activation remains a Contract Service operation that
updates the single authoritative mapping.

## Decision observation boundary

A `RuntimeDecision` proves that one active executable produced a contract-valid
result. It does not prove that the application used the result.

```text
RuntimeDecision
  -> SDK emits flaggo.decision.received through OpenTelemetry
  -> application may apply or render result
  -> selected application telemetry may correlate to that decision
```

The built-in observation records receipt, not application. It carries the
decision, contract, executable, result, and evaluation identities needed by
later analysis, but it does not prove that the application used the result.

The optional current-exposure input describes a previous applied decision in
the SDK's current activity context. It does not select the new executable and
does not prove exposure of the newly returned result.

## Failure and retry boundary

The service returns explicit errors for unknown or mismatched contract
identity, invalid complete input, missing or inconsistent activation,
executable integrity failure, evaluator failure, invalid result, throttling,
and unavailable required dependencies.

Each retry is a new stateless request and resolves the then-active executable.
Correlation and retry timing use HTTP headers; machine-readable failure
identity uses RFC 9457 Problem Details type URIs.

The server never turns these failures into a successful contract-default
result. Contract no-match default behavior remains a successful evaluation;
SDK-local outage fallback remains a separate deferred concern.

## Invariants

1. Application deployment supplies an exact contract name and digest.
2. The SDK constructs and retries one complete logical input.
3. Authentication establishes tenant/application/environment scope.
4. The service resolves exactly one active immutable executable per request.
5. Management current-version and candidate-generation order never select
   runtime authority.
6. Runtime generation, evidence analysis, and activation never occur in the
   request path.
7. Evaluation depends only on the resolved executable and complete input.
8. The service retains no semantic per-request state.
9. A retry may observe a newly activated executable.
10. Returning a decision does not imply exposure.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Decision authority](AUTHORITY.md)
- [Evidence and learning](EVIDENCE.md)
- [Runtime evaluation model](../contracts/RUNTIME_EVALUATION.md)
- [Runtime API v3](../../../contracts/openapi/flaggo-runtime-v3.yaml)
