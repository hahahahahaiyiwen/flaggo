# Architecture overview

## Status

This document defines the target Flaggo architecture. It replaces the earlier
definition-bundle, target-resolution, generic State Store, synchronous
evidence-input, durable-decision-record, and explicit exposure-confirmation
model.

The v3 OpenAPI documents and JSON Schemas under `contracts/` are authoritative
for wire behavior. The documents under `docs/design/contracts/` define the
contract, executable, lifecycle, and runtime semantics used by this
architecture.

## Architectural foundations

The architecture starts from these product requirements:

1. Application code requests one exact immutable version of a named decision.
2. A runtime result is determined only by one immutable active executable and
   one complete runtime input.
3. Contract acceptance, executable generation, and activation are different
   transitions with different outcomes.
4. Evidence-based generation may be slow or nondeterministic, but runtime
   evaluation must remain bounded and deterministic.
5. Returning a decision does not establish that the application applied it.
6. Applications using an older contract digest must continue to use that
   digest's active executable after a newer version is accepted.

These requirements produce the core model:

```text
DecisionContract
  -> Contract Acceptance
  -> Executable Generation
  -> CandidateExecutable
  -> Activation
  -> ActiveExecutable

ActiveExecutable + RuntimeInput
  -> deterministic RuntimeDecision
```

An accepted contract establishes an immutable interface and required default.
Generation creates immutable candidates. Activation alone selects executable
runtime authority.

## System context

[Application boundaries and lifecycle](APP_BOUNDARIES.md) is the canonical
end-to-end map. It distinguishes client activity, Web APIs, background workers,
and stores; defines every app's responsibilities and prohibited
responsibilities; and documents store ownership, authority transitions,
failure isolation, and instance-multiplicity constraints.

An implementation may co-locate services or stores. The logical ownership and
request-path boundaries remain the same even when deployment units are
combined.

## Component responsibilities

### Application and SDK

The application chooses the exact `contractName` and `contractDigest` with
which it was built. Application values are bound to declared contract
attributes through the SDK.

The SDK:

- constructs the complete `RuntimeInput`;
- adds reserved internal attributes, initially `_random`;
- reuses those internal values for transport retries of one logical
  evaluation;
- may carry the previous applied exposure as `currentExposure`;
- sends the exact-version runtime request; and
- emits `flaggo.decision.received` through the application's OpenTelemetry
  pipeline after a successful response.

Contract attributes, including user or principal identifiers, are decision
data. They never establish authentication, authorization, application scope,
or environment scope.

### Contract Service

The Contract Service owns the management resource and executable lifecycle:

- validate and accept a named `DecisionContract`;
- canonicalize semantic content and compute `contractDigest`;
- persist immutable accepted versions;
- generate and activate the required default executable before reporting the
  version ready;
- generate authored executables and receive evidence-generated candidate
  proposals from Async Analysis;
- validate immutable candidates against their exact contract;
- atomically activate one executable for a contract digest in an authenticated
  tenant/application/environment scope.

The management resource may expose a current version by name. That pointer is
for authoring and discovery only; the Decision Service never follows it.

### Executable generation and learning

Executable Generation has three inputs but one output type:

```text
contract default ----------------------\
authored executable --------------------+-> immutable DecisionExecutable
contract + correlated evidence --------/
```

Default and authored generation may run as Contract Service capabilities or
dedicated workers. Evidence-based analysis is asynchronous and consumes
evidence outside the runtime path.

Every candidate records its exact `contractDigest` and generation provenance.
Async Analysis may propose a candidate but cannot write runtime authority
directly. The candidate returns through Contract Service, which validates it
and applies its activation policy.

The initial evidence-based policy is `mode: auto-activation`. For a valid
candidate from the current contract, the Contract Service attempts an
atomic activation immediately. This policy does not make runtime search for
the latest generated artifact; runtime still reads only the activation index.

### Decision Service

The Decision Service owns one stateless runtime operation:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

For each request it authenticates tenant/application/environment scope, verifies the
name and digest, validates the SDK-constructed input, resolves the active
immutable executable, evaluates it, validates the result, and returns a
`RuntimeDecision`.

It does not:

- select a current or latest contract version;
- generate or activate executables;
- query evidence or run analysis;
- resolve a generic target hierarchy;
- apply a separate generic constraints layer;
- persist an evaluation session, idempotency record, or durable decision
  record; or
- decide whether the application exposed the result.

### Evidence path

Runtime traffic and evidence traffic are separate. The SDK emits the built-in
`flaggo.decision.received` observation, while applications emit ordinary
OpenTelemetry metrics, logs, spans, and span events. Each current contract's
required evidence sources select relevant application telemetry and map
declared correlation attributes.

The Evidence Store supports later analysis and provenance. It is not a
synchronous operand store for Decision Service requests.

## Data ownership

The complete ownership matrix, including Raw OTLP Inbox, materializer
checkpoint and catalog cache, Evidence Store, and planned analysis provenance,
is defined in
[Application boundaries and lifecycle](APP_BOUNDARIES.md#durable-state-ownership).
The core online authority data is:

| Data | Owner | Mutability | Runtime role |
| --- | --- | --- | --- |
| Accepted `DecisionContract` version | Contract Service / Contract Store | Immutable by `contractDigest` | Validates input and result for the requested digest |
| Management current-version pointer | Contract Service | Mutable by name | Selects the version for management reads and new evidence analysis; runtime never resolves it |
| `DecisionExecutable` and provenance | Contract Service / Executable Store | Immutable by `executableDigest` | Supplies bounded behavior |
| Executable lifecycle state | Contract Service / Executable Store | Atomic Candidate/Active/Inactive transition per scope and `contractDigest` | Selects the only executable with runtime authority |
| Runtime input | Application and SDK | Per logical evaluation | Complete explicit evaluator input |
| Runtime decision | Decision Service | Response value; not retained by the semantic runtime contract | Returned to the caller |
| Decision observations and selected application telemetry | SDK, application, and evidence pipeline | Append-oriented observations | Asynchronous learning only |

## Online and asynchronous dependency boundaries

The online decision path depends only on:

- authenticated scope;
- the requested accepted contract version;
- its current activation;
- the referenced immutable executable; and
- the complete runtime input.

Evidence ingestion, learning schedules, candidate generation, activation
workflows, and management current-version lookup are not online runtime
dependencies. Their delay or failure cannot change the meaning of an
in-progress evaluation. If a replacement cannot be generated or activated,
the existing executable remains active.

## Cross-version behavior

Accepting a new digest creates a new runtime authority slot rather than
mutating the old one:

```text
name = tetris.dropInterval

D1 -> RuntimeActivation[scope, D1] = E1
D2 -> RuntimeActivation[scope, D2] = DefaultExecutable(D2)

ManagementCurrent[scope, name] = D2
```

Applications carrying `D1` continue to evaluate `E1`. Applications carrying
`D2` begin with the default executable and later observe replacements
activated for `D2`. Flaggo does not infer compatibility or migrate runtime
authority across digests.

## Architecture invariants

1. A contract name identifies a logical management resource; a digest
   identifies one exact immutable version.
2. Every accepted-ready contract digest has an active default executable.
3. Every executable is immutable and bound to one exact contract digest.
4. Activation is the sole source of runtime executable authority.
5. Runtime evaluates one active executable against one complete explicit
   input and retains no semantic request state.
6. Evidence-based generation never runs in the decision request path.
7. A newer digest does not deactivate or reinterpret an older digest.
8. Authentication establishes tenant/application/environment scope; contract
   attributes never do.
9. A returned decision and the built-in decision-received observation do not
   prove that the application applied the result.

## Related documents

- [Application boundaries and lifecycle](APP_BOUNDARIES.md)
- [Contract clients and Contract Service](CONTRACT_SERVICE.md)
- [Decision authority](AUTHORITY.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Evidence and learning](EVIDENCE.md)
- [Decision contracts and executables](../contracts/CONTRACTS.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
- [Runtime evaluation model](../contracts/RUNTIME_EVALUATION.md)
