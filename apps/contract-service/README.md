# Contract Service

**Role:** implemented synchronous Web API and management authority.

Contract Service is the composition root for contract acceptance and
executable lifecycle management. It establishes immutable contract identity,
creates the required default executable, and is the only app allowed to grant
runtime authority through activation.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Validate, canonicalize, and accept complete `DecisionContract` versions.
- Establish global ownership of each contract name by its first accepted
  declared authority.
- Compute `contractDigest` and persist immutable accepted versions.
- Generate and activate the required default executable before reporting a
  version ready.
- Generate deterministic authored executables.
- Validate analysis Candidates against their exact current accepted contract.
- Persist inactive Candidate artifacts and durable workspace-cycle provenance
  idempotently.
- Atomically activate one executable for each contract digest.
- Publish the current contract catalog used by Evidence Materializer.

## Boundaries

Contract Service does not:

- evaluate runtime decision requests;
- construct application `RuntimeInput`;
- ingest or materialize OpenTelemetry;
- query Evidence Store or correlate evidence;
- schedule or perform asynchronous evidence analysis; or
- allow a candidate producer to write activation state directly.

Async Analysis submits immutable Candidate rules and trusted cycle provenance
through Contract Service. Contract Service derives contract identity from the
route, recompiles the rules, rejects stale current digests, and persists only
`Candidate` lifecycle state. Candidate activation is a separate Contract
Service operation and is not part of this boundary.

## Interfaces

The implemented HTTP surface is:

```text
POST /v3/decision-contracts/{contractName}/validate
PUT  /v3/decision-contracts/{contractName}
GET  /v3/decision-contracts/{contractName}
GET  /v3/decision-contracts/{contractName}/versions
GET  /v3/decision-contracts/{contractName}/versions/{contractDigest}
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/candidates
GET  /v3/decision-contract-catalog/current
GET  /health/live
GET  /health/ready
```

The Management API currently has no authentication boundary. Declared
authority is persisted contract data used for name ownership, telemetry
routing, and evidence identity; it is not a caller-supplied lookup selector.
Future authentication must authorize an already loaded resource rather than
derive its storage identity from credentials.

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

## Operational observability

The service uses instrumentation scope `flaggo.contract-service`. Automatic
ASP.NET Core and HTTP client telemetry is supplemented by domain spans and
bounded metrics for validation, deployment, reads, catalog reads, Candidate
admission, and activation. Important spans and lifecycle logs carry exact
contract, Candidate, and executable identities; metrics never use those exact
identifiers as dimensions.

Console logging remains enabled, and structured JSON formatting can be selected
through standard .NET logging configuration. Logs, metrics, and traces export
as OTLP/HTTP binary Protobuf only when standard
`OTEL_EXPORTER_OTLP_*_ENDPOINT` configuration is present.

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
- [Internal service observability](../../docs/design/architecture/OBSERVABILITY.md)
