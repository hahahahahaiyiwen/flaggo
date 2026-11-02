# Decision Service

**Role:** implemented synchronous Web API for exact-version runtime
evaluation.

Decision Service is the composition root for one bounded runtime operation. It
loads the exact named contract version, resolves the executable active for the
requested contract digest, evaluates that immutable executable against complete
input, and returns a `RuntimeDecision`.

The canonical cross-app lifecycle is documented in
[Application boundaries and lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

## Responsibilities

- Verify that the route name owns the requested `contractDigest`.
- Validate complete SDK-constructed `RuntimeInput`.
- Resolve the one active executable for the exact digest.
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

The Runtime API currently has no authentication boundary. The route's
`contractName + contractDigest` identifies the exact accepted contract, and
`contractDigest` identifies its activation. Future authentication must
authorize that already loaded resource rather than select it.

## State and dependencies

| Boundary | Access | Purpose |
| --- | --- | --- |
| Contract Store | Read | Verify exact accepted identity and validate input and result |
| Executable Store | Read | Resolve the active lifecycle entry and immutable executable |
| Decision and expression modules | In-process | Execute already checked deterministic behavior |

Decision Service owns no semantic durable state. Both stores currently use the
configured SQLite database, selected with `FLAGGO_DECISION_DATABASE_PATH`; the
default is `flaggo.db`.

Readiness requires Contract Store and Executable Store only. It does not
depend on Contract Service availability, OTel Ingestion, Evidence Materializer,
Evidence Store, or Async Analysis.

## Operational observability

The service uses instrumentation scope `flaggo.decision-service`. Each
exact-version evaluation emits `flaggo.decision.evaluate`, correlates the
contract and executable digests on the span, and records bounded success,
validation, not-found, dependency, integrity, and evaluation outcomes.
Evaluation counters and duration histograms use only bounded outcome,
evaluation-source, and failure-category dimensions.

Structured JSON console logging remains enabled. Logs, metrics, and traces export
as OTLP/HTTP binary Protobuf only when standard
`OTEL_EXPORTER_OTLP_*_ENDPOINT` configuration is present.

## Execution model

The architecture diagram represents Decision Service as a rectangle because it
is a request-driven Web API. The node is a conceptual app boundary, not a
single process or replica. The request path is stateless, so any replica may
serve a request when it can coherently read the contract and executable stores.

## Run

From the repository root:

The Rust composition owns these three routes, the Decision runtime and both
SQLite readers. All launchers use it; the superseded .NET sources and build
paths are removed.

```powershell
$env:FLAGGO_DECISION_DATABASE_PATH = 'C:\data\flaggo.db'
$env:FLAGGO_DECISION_LISTEN_ADDRESS = '127.0.0.1:8081'
cargo run --locked -p flaggo-decision-service
```

The Rust database setting is a filesystem path, not a .NET connection string.
Its default is `flaggo.db`; the listen default is `127.0.0.1:8081`.
The former `ConnectionStrings__Flaggo` and `ASPNETCORE_URLS` settings are not consumed.
When upgrading a local environment, explicitly carry its database path to both
Rust services and update its listen settings; no state reset or data-copy step is needed.
Legacy settings without their explicit Rust replacements fail startup with a
configuration error before opening a database, rather than silently selecting
the default path or port. Explicit Rust settings take precedence when both exist.
Startup initializes the existing versioned schemas non-destructively and fails
explicitly for unavailable or unsupported databases.

The Rust process reuses the service-observability module and standard OTLP/HTTP
Protobuf exporter configuration. It emits HTTP spans/durations and the registered
decision counter/histogram with bounded dimensions, propagates W3C trace context,
echoes validated correlation IDs, and keeps exact identities on operation spans
only. Shutdown drains HTTP requests for at most 30 seconds, closes both pools
and flushes telemetry; Unix also handles SIGTERM.

## Verify

Focused verification includes a real spawned Rust process exporting all three
signals to a test OTLP receiver, alongside router tests for semantic, native JSON,
integrity, dependency, evaluation, result-schema and health failures:

```powershell
cargo test --locked -p flaggo-service-hosting -p flaggo-decision-service
```

The live .NET/Rust comparison is opt-in because it requires the unchanged .NET
applications to be built from an explicit pinned historical checkout.
It checks both Rust- and .NET-produced SQLite databases,
including actual .NET Contract Service deployment, route aliases, error envelopes,
health, media types and exact known-length/chunked request-size boundaries.

Use the [combined historical acceptance runner](../../tools/core-parity/README.md#opt-in-live-http-acceptance)
with an explicit `--reference-root`.

Dynamic health timestamps and package versions are validated independently.
Native JSON diagnostic suffixes remain serializer-specific; their problem type,
status and invalid-body prefix are compared. Other tested response fields are
compared without dropping runtime, checked-artifact or lifecycle identities.
Contract management/activation is composed separately in the Rust Contract
Service. The opt-in `npm run test:tetris-full-loop` gate exercises the
unchanged TypeScript SDK and application through the real telemetry,
materialization, deterministic analysis, Candidate and automatic-activation
loop, then verifies SDK/REST decisions and exact authority/provenance after
restart. It builds and runs without .NET; see the
[full-loop instructions](../../examples/tetris-integration/README.md#rust-full-loop-acceptance).
The [combined migration acceptance command](../../tools/core-parity/README.md#opt-in-live-http-acceptance)
includes both live HTTP comparisons. .NET is not a normal build or test prerequisite.

## Related documents

- [Runtime client and Decision Service](../../docs/design/architecture/RUNTIME.md)
- [Decision authority](../../docs/design/architecture/AUTHORITY.md)
- [Runtime evaluation model](../../docs/design/contracts/RUNTIME_EVALUATION.md)
- [Runtime API v3](../../contracts/openapi/flaggo-runtime-v3.yaml)
- [Internal service observability](../../docs/design/architecture/OBSERVABILITY.md)
