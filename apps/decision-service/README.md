# Decision Service

**Role:** implemented synchronous Web API for exact-version runtime
evaluation.

Decision Service is the composition root for one bounded runtime operation. It
authenticates an application scope, resolves the executable active for the
requested contract digest, evaluates that immutable executable against complete
input, and returns a `RuntimeDecision`.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Authenticate and authorize runtime requests.
- Verify that the route name owns the requested `contractDigest`.
- Validate complete SDK-constructed `RuntimeInput`.
- Resolve the one active executable for the authenticated scope and exact
  digest.
- Verify executable integrity and perform bounded deterministic evaluation.
- Validate the result against the accepted contract.
- Return `RuntimeDecision` or an explicit Problem Details failure.
- Report liveness and the availability of required stores.

## Boundaries

Decision Service does not:

- select a current or latest contract version;
- accept contracts or generate, validate, or activate executables;
- bind application attributes or apply a returned result;
- ingest telemetry, query evidence, or run analysis;
- provide SDK-local fallback; or
- persist runtime requests, responses, sessions, or exposure records.

Runtime evaluation never waits for ingestion, materialization, evidence
freshness, or analysis.

## Interfaces

The implemented HTTP surface is:

```text
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
GET  /health/live
GET  /health/ready
```

The Runtime API requires the `flaggo.decisions:decide` scope. Development
configuration provides a local authentication bypass; it is not a production
security model.

## State and dependencies

| Boundary | Access | Purpose |
| --- | --- | --- |
| Contract Store | Read | Verify exact accepted identity and validate input and result |
| Executable Store | Read | Resolve the active lifecycle entry and immutable executable |
| Decision and expression modules | In-process | Execute already checked deterministic behavior |

Decision Service owns no semantic durable state. Both stores currently use the
configured SQLite database, selected with `ConnectionStrings__Flaggo`; the
default is `Data Source=flaggo.db`.

Readiness requires Contract Store and Executable Store only. It does not
depend on Contract Service availability, OTel Ingestion, Evidence Materializer,
Evidence Store, or Async Analysis.

## Execution model

The architecture diagram represents Decision Service as a rectangle because it
is a request-driven Web API. The node is a conceptual app boundary, not a
single process or replica. The request path is stateless, so any replica may
serve a request when it can coherently read the authority stores.

## Run

From the repository root:

```powershell
dotnet run --project apps\decision-service\src\Flaggo.DecisionService\Flaggo.DecisionService.csproj
```

## Verify

```powershell
dotnet build Flaggo.slnx -c Debug --no-restore
```

## Related documents

- [Runtime client and Decision Service](../../docs/design/architecture/RUNTIME.md)
- [Decision authority](../../docs/design/architecture/AUTHORITY.md)
- [Runtime evaluation model](../../docs/design/contracts/RUNTIME_EVALUATION.md)
- [Runtime API v3](../../contracts/openapi/flaggo-runtime-v3.yaml)
