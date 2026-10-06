# Contract clients and Contract Service

## Purpose

This document describes the management path from a contract author or CI
client to an accepted, runtime-ready contract version. It defines client and
Contract Service responsibilities, service dependencies, transactional
boundaries, and failure behavior.

It does not redefine `DecisionContract` or `DecisionExecutable`. Their shapes,
identities, canonicalization rules, and semantic invariants belong to
[Decision contracts and executables](../contracts/CONTRACTS.md). Lifecycle
transitions belong to
[Decision contract lifecycle](../contracts/LIFECYCLE.md), and the Management
API v3 OpenAPI document is the wire-level authority.

The Contract Service app boundary and its relationship to every other app are
canonical in
[Application boundaries and lifecycle](APP_BOUNDARIES.md).

## System context

```text
contract author
  -> source-controlled DecisionContract definition
  -> deployment authority binding
  -> management client or CI
  -> Management API v3
  -> Contract Service
       -> Contract Store
       -> executable generator
       -> Executable Store

Evidence Store -> Async Analysis (planned)
                    -> immutable candidate + provenance
                    -> Contract Service validation and activation
```

The management client supplies the complete authority-bound deployed contract.
The Contract Service establishes accepted identity and runtime readiness.
During deployment, a client does not assert a `contractDigest`,
`executableDigest`, or activation record as trusted authority; it may use
server-returned digests for later exact reads and application configuration.

## Contract source and deployment files

An authored `DecisionContract` definition is contract-as-code rather than
environment configuration. Keep one independently versioned definition per
file using:

```text
flaggo/contracts/<decision-name>.decision-contract.json
```

The file name must preserve the exact embedded contract `name`. Service URLs,
credentials, and deployment authority do not belong in the authored contract
file.

Projects declare one deployment authority and their contract inventory in
`flaggo.deploy.json`:

```json
{
  "format": "flaggo.deploy/v2",
  "authority": {
    "tenant": "local",
    "application": "checkout",
    "environment": "production"
  },
  "contracts": [
    "flaggo/contracts/checkout.shippingMethod.decision-contract.json",
    "flaggo/contracts/search.pageSize.decision-contract.json"
  ]
}
```

Paths are portable forward-slash relative paths contained within the
manifest's directory. The manifest does not embed contracts and is not an
atomic multi-contract API payload. Deployment tooling processes each referenced
contract independently. It injects the manifest authority before validation,
digest calculation, and the name-keyed `PUT`. Authority is therefore part of
the complete deployed contract and `contractDigest`, while the authored
definition remains portable. Until authenticated ingress exists, authority is
declared rather than security-derived.

## Client responsibilities

A contract author or CI client:

- maintains an authority-free contract definition as source-controlled input;
- binds manifest authority into the complete deployed `DecisionContract`;
- addresses the logical management resource by `contractName`;
- may use dry-run validation independently when early feedback is useful;
- submits the complete desired contract rather than an incremental mutation;
- records each returned `contractDigest` in generated runtime configuration;
- uses exact-version reads for audit or reconstruction; and
- treats the name-level current version as management discovery, never as a
  runtime selection mechanism.

After all contracts are accepted, deployment tooling generates one immutable
`flaggo.runtime-config/v1` document containing the manifest authority, Contract
Service, Decision Service, and OTel Ingestion URLs, and exact
`{ contractName, contractDigest }` bindings. Decision and telemetry SDK
composition consume that same artifact; runtime never resolves a moving
name-level version.

Build and deployment are separate:

```text
Build
  -> generate or validate local artifacts
  -> compile application code
  -> perform no service mutation

Deploy
  -> load one authored DecisionContract definition
  -> bind manifest AuthorityScope
  -> validate its wire shape locally
  -> PUT it to Contract Service
  -> receive contractDigest and activeExecutableDigest
  -> generate and distribute the immutable runtime configuration
```

The SDK validates the wire shape before sending `PUT`. Contract Service then
performs authoritative semantic validation as part of deployment. Calling the
remote dry-run validation endpoint first is optional and is not part of the
normal deployment flow.

Clients may retry a `PUT` with identical semantic content. They must not infer
that a transport timeout means deployment failed; the follow-up response or
exact-version read establishes the durable outcome.

## Management API boundary

The Contract Service exposes:

```http
POST /v3/decision-contracts/{contractName}/validate
PUT  /v3/decision-contracts/{contractName}

GET  /v3/decision-contracts/{contractName}
GET  /v3/decision-contracts/{contractName}/versions
GET  /v3/decision-contracts/{contractName}/versions/{contractDigest}
GET  /v3/decision-contract-catalog/current
```

### Dry-run validation

`POST .../validate` applies the same contract validation and canonical digest
calculation used by deployment but performs no durable mutation:

```text
DecisionContract
  -> verify route name equals payload name
  -> validate syntax and semantics
  -> canonicalize semantic content
  -> compute proposed contractDigest
  -> return valid or invalid result
```

A semantically invalid contract is a successful validation operation whose
body reports `status: invalid`. Malformed transport input, unsupported media
types, and other protocol failures use Problem Details instead.

Validation does not reserve a digest, create a version, generate an
executable, change a current pointer, or activate runtime authority.

### Deployment

`PUT .../{contractName}` accepts one complete immutable version and makes it
runtime-ready:

```text
read AuthorityScope from the complete contract
  -> verify route name equals payload name
  -> validate and canonicalize DecisionContract
  -> compute contractDigest
  -> find or persist immutable contract version
  -> generate and validate DefaultExecutable
  -> persist immutable executable
  -> atomically activate default through IExecutableStore
  -> move ManagementCurrent[contractName]
  -> return ready DecisionContractVersion
```

The first accepted version also establishes
`ContractResource[contractName] = declared AuthorityScope`. A later deployment
of the same name with another declared authority fails with
`contract-name-authority-conflict`.

A newly created semantic version returns `201` and the exact-version
`Location`. Repeating an already accepted semantic version returns `200` and
the existing version. Non-semantic-only changes do not create another digest.

The externally visible acceptance boundary includes default activation. The
service may use internal staging records, but it must not expose a contract
version as `ready` before its default executable is durable and active.

### Reads

The name-level `GET` returns the version selected by
`ManagementCurrent[contractName]`. It is an authoring and discovery
convenience.

The versions collection returns immutable version summaries in stable
cursor-paginated order. The exact-version operation returns one immutable
version only when its digest belongs to the named resource.

Neither `ManagementCurrent` nor version-list ordering grants runtime
authority. The Decision Service accepts an exact digest and reads
`RuntimeActivation` instead.

The cross-authority current-contract catalog returns every complete current
deployed contract for Evidence Materializer route compilation. Its ETag is
opaque conditional-fetch state, not contract or evidence identity.

## Contract Service responsibilities

The Contract Service owns:

- route and payload identity validation;
- global contract-name ownership by declared authority;
- bounded contract syntax and semantic validation;
- canonicalization and `contractDigest` computation;
- immutable contract-version persistence;
- default and authored executable generation orchestration;
- candidate validation and immutable executable persistence;
- atomic activation;
- management current-version transitions;
- receiving evidence-generated candidate proposals through an explicit app
  boundary and resolving them with a paced activation worker; and
- management response and Problem Details mapping.

It does not evaluate runtime decision requests, construct `RuntimeInput`,
collect application evidence synchronously, schedule evidence analysis, or
infer contract compatibility across digests.

## Service dependencies and owned records

| Dependency or record | Contract Service use |
| --- | --- |
| Contract Store | Persist and read immutable accepted versions by scope, name, and digest |
| Executable Store | Persist immutable generated executables and provenance; atomically manage scoped Candidate, Active, and Inactive lifecycle state |
| Management current pointer | Select the version returned by the name-level management read and eligible for new evidence-based analysis |
| Generation capability | Produce default and authored candidates without granting runtime authority |
| Candidate ingress and activation worker | Receive validated evidence-generated Candidates and provenance from Async Analysis, then resolve eligible Candidates without granting the producer activation authority |

The physical database layout is an implementation choice. These records have
different semantic roles even when stored transactionally in one database.

## Generation and activation orchestration

### Default executable

Every accepted contract has a required literal default. Deployment generates
and activates its default executable before returning a ready version. Failure
to persist or activate the default prevents successful deployment.

### Authored executable

When the accepted contract contains authored source, the service invokes the
appropriate compiler or generator. A successfully generated candidate is
validated, persisted, and automatically activated. Generation or activation
failure leaves the already active default executable unchanged.

Condition-only authored rules may require asynchronous generation. Their
detailed generation protocol remains deferred, but their output crosses the
same candidate-validation and activation boundary.

### Evidence-generated executable

Async Analysis owns interval scheduling, evidence selection, correlation, and
evidence-based generation. It reads exact current contracts through a Contract
Service boundary and may return an immutable candidate plus provenance, but it
cannot write runtime authority. Candidate submission validates and persists the
immutable executable as `candidate`; it does not activate it.

The initial policy is `mode: auto-activation`. An internal hosted worker runs
one bounded scan during service startup, then waits
`FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS` between scans. The positive
default is 5000 ms and is independent of
`learning.policy.evaluate.interval`.

Discovery includes only executables with durable Async Analysis admission
provenance. For each contract, the current digest is preferred and the newest
pending admission is selected. A successful activation makes older pending
Candidates inactive. Stale, invalid, and policy-ineligible Candidates also
become inactive; retryable infrastructure failures remain `candidate`.
An exact admission retry returns the original admission and its current
lifecycle even if the worker already resolved it; the retry never reopens or
mutates that executable.

Auto-activation is a Contract Service transition:

```text
admitted Candidate
  -> immutable contract and executable validation
  -> current pointer + Candidate version + active version compare
  -> atomic lifecycle replacement or yield
```

It is not a rule telling the Decision Service to query the newest generated
executable.

## Consistency and concurrency

The Contract Service must preserve these ordering guarantees:

- accepted contract and executable content are immutable by digest;
- deployment of identical semantic content converges on one version;
- a ready response is impossible before default activation succeeds;
- current-version movement occurs only after the new digest is ready;
- accepting a new digest does not change older digest activations;
- a superseded learning run cannot activate after the current contract changes;
- current-pointer comparison and executable lifecycle updates occur in one
  non-deferred SQLite transaction;
- Candidate and active-executable lifecycle versions prevent ABA and stale
  scan snapshots from changing authority;
- concurrent Contract Service workers need no leader: one conditional
  transition wins and conflicting workers yield until a later scan;
- only one complete executable digest occupies an activation slot;
- duplicate activation of the same executable is idempotent; and
- a failed replacement leaves the prior activation intact.

Transactions, conditional writes, and retry mechanisms may implement these
guarantees. Their storage-specific representation is not part of the public
contract.

## Failure boundary

The service returns explicit failures for malformed requests, route/payload
mismatch, contract-name authority conflict, unsupported contract features,
persistence failure, default generation failure, and activation failure.
Failure to generate a later authored or learned candidate leaves the current
executable active. Executable lifecycle state is the durable authority record;
activation success, rejection, supersession, yield, and failure are reported
through spans, metrics, and structured events rather than a separate outcome
journal.

Dry-run semantic errors are returned as structured validation results.
Protocol and service failures use RFC 9457 Problem Details. The service never
returns a ready version with missing activation and never silently reuses an
executable from another contract digest.

## Invariants

1. Clients submit complete desired contract content under a stable name.
2. The server computes semantic digests; clients do not assert trusted
   identities.
3. Validation is side-effect free.
4. Deployment is idempotent by canonical semantic content.
5. A deployed `ready` version has a durable active default executable.
6. Generation creates immutable candidates; only activation grants runtime
   authority.
7. `auto-activation` performs a checked atomic activation rather than
   deploying by generation recency.
8. The management current-version pointer never selects a runtime contract
   version.
9. Older digest activations survive acceptance of a newer version.
10. Multi-instance activation conflicts yield without a tight retry or worker
    failure.
11. Runtime evaluation, application result use, and telemetry emission remain
    outside Contract Service.

## Related documents

- [Application boundaries and lifecycle](APP_BOUNDARIES.md)
- [Contract Service app](../../../apps/contract-service/README.md)
- [Architecture overview](OVERVIEW.md)
- [Decision authority](AUTHORITY.md)
- [Runtime client and Decision Service](RUNTIME.md)
- [Evidence and learning](EVIDENCE.md)
- [Decision contracts and executables](../contracts/CONTRACTS.md)
- [Decision contract lifecycle](../contracts/LIFECYCLE.md)
- [Management API v3](../../../contracts/openapi/flaggo-management-v3.yaml)
