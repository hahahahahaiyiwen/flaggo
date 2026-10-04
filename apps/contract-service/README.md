# Contract Service

**Role:** implemented synchronous Web API and management authority.

Contract Service is the composition root for contract acceptance and
executable lifecycle management. It establishes immutable contract identity,
creates the required default executable, and is the only app allowed to grant
runtime authority through activation.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Authenticate and authorize Management API operations.
- Validate, canonicalize, and accept complete `DecisionContract` versions.
- Compute `contractDigest` and persist immutable accepted versions.
- Generate and activate the required default executable before reporting a
  version ready.
- Generate deterministic authored executables.
- Validate immutable candidates against their exact accepted contract.
- Persist executable artifacts and generation provenance.
- Atomically activate one executable for each authority scope and contract
  digest.
- Publish the current contract catalog used by Evidence Materializer.

## Boundaries

Contract Service does not:

- evaluate runtime decision requests;
- construct application `RuntimeInput`;
- ingest or materialize OpenTelemetry;
- query Evidence Store or correlate evidence;
- schedule or perform asynchronous evidence analysis; or
- allow a candidate producer to write activation state directly.

The planned Async Analysis app will submit immutable candidates and provenance
through a Contract Service boundary. That boundary is not implemented yet.
Contract Service will retain candidate validation and activation authority.

## Interfaces

The implemented HTTP surface is:

```text
POST /v3/decision-contracts/{contractName}/validate
PUT  /v3/decision-contracts/{contractName}
GET  /v3/decision-contracts/{contractName}
GET  /v3/decision-contracts/{contractName}/versions
GET  /v3/decision-contracts/{contractName}/versions/{contractDigest}
GET  /v3/decision-contract-catalog/current
GET  /health/live
GET  /health/ready
```

The Management API authenticates validation, acceptance, reads, and
materialization-catalog access with separate scopes. Development configuration
provides a local authentication bypass; it is not a production security model.

## State and dependencies

| Boundary | Access | Purpose |
| --- | --- | --- |
| Contract Store | Owner and writer | Immutable accepted versions and the management current-version pointer |
| Executable Store | Owner and writer | Immutable executables, provenance, lifecycle state, and atomic activation |
| Domain modules | In-process | Contract validation, canonicalization, expression compilation, and lifecycle orchestration |

Both stores currently use the configured SQLite database. Physical
co-location does not merge their semantic ownership. Configure the connection
with `ConnectionStrings__Flaggo`; the default is `Data Source=flaggo.db`.

Readiness requires both owned stores. It does not depend on Decision Service,
OTel Ingestion, Evidence Materializer, Evidence Store, or Async Analysis.

## Execution model

The architecture diagram represents Contract Service as a rectangle because it
is a request-driven Web API. The node is a conceptual app boundary, not a
single process or replica. Replication is valid only when every instance shares
the same authoritative stores and preserves lifecycle concurrency invariants.

## Run

From the repository root:

```powershell
dotnet run --project apps\contract-service\src\Flaggo.ContractService\Flaggo.ContractService.csproj
```

## Verify

```powershell
dotnet build Flaggo.slnx -c Debug --no-restore
```

## Related documents

- [Contract clients and Contract Service](../../docs/design/architecture/CONTRACT_SERVICE.md)
- [Decision authority](../../docs/design/architecture/AUTHORITY.md)
- [Decision contract lifecycle](../../docs/design/contracts/LIFECYCLE.md)
- [Management API v3](../../contracts/openapi/flaggo-management-v3.yaml)
