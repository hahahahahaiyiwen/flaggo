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
co-location does not merge their semantic ownership. Configure the filesystem
path with `FLAGGO_CONTRACT_DATABASE_PATH`; the default is `flaggo.db`.

Readiness requires both owned stores. It does not depend on Decision Service,
OTel Ingestion, Evidence Materializer, Evidence Store, or Async Analysis.

## Operational observability

The service uses instrumentation scope `flaggo.contract-service`. Automatic
HTTP request telemetry is supplemented by domain spans and
bounded metrics for validation, deployment, reads, catalog reads, Candidate
admission, and activation. Important spans and lifecycle logs carry exact
contract, Candidate, and executable identities; metrics never use those exact
identifiers as dimensions.

Structured JSON console logging remains enabled. Logs, metrics, and traces export
as OTLP/HTTP binary Protobuf only when standard
`OTEL_EXPORTER_OTLP_*_ENDPOINT` configuration is present.

## Execution model

The architecture diagram represents Contract Service as a rectangle because it
is a request-driven Web API. The node is a conceptual app boundary, not a
single process or replica. Replication is valid only when every instance shares
the same authoritative stores and preserves lifecycle concurrency invariants.

## Run

From the repository root:

The Rust host implements the same management routes, health checks, immutable
acceptance, default/authored readiness, catalog ETag, pagination, Candidate
admission, manual compare-and-swap activation, and paced automatic activation.
All launchers use this host; the superseded .NET sources and build paths are removed.

```powershell
$env:FLAGGO_CONTRACT_DATABASE_PATH = "flaggo.db"
$env:FLAGGO_CONTRACT_LISTEN_ADDRESS = "127.0.0.1:8080"
cargo run --locked -p flaggo-contract-service
```

These are a filesystem path and a socket address. The former .NET
`ConnectionStrings__Flaggo` and `ASPNETCORE_URLS` settings are not consumed.
When upgrading a local environment, explicitly carry its database path to both
Rust services and update its listen settings; no state reset or data-copy step is needed.
Legacy settings without their explicit Rust replacements fail startup with a
configuration error before opening a database, rather than silently selecting
the default path or port. Explicit Rust settings take precedence when both exist.
With no legacy settings, omitting them uses `flaggo.db` and `127.0.0.1:8080`.
Startup validates configuration before opening
the database and initializes both existing schemas without resetting state.
`FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS` defaults to 5000; blank uses the
default, otherwise it must be a positive, unsigned decimal integer within the
TimeSpan/platform timer range. The worker scans once before serving, then
processes at most 100 contract names per paced scan. Scan/candidate failures
are logged; an individual failure does not stop other candidates.

Automatic activation deliberately uses **validated interoperability**, not
recompilation-byte identity. It recompiles the public source and verifies the
executable digest, then strictly materializes the original checked artifact
against the accepted contract. Source/AST binding, checked checksums,
referenced attributes, types, and bounded-profile constraints remain enforced.
Valid .NET-produced protobuf can differ from Rust compilation bytes; it is
neither rejected for that difference nor rewritten. This is an approved
deviation from the .NET worker's byte-equality check, not a claim that the
unchanged .NET worker will automatically activate Rust-produced artifacts.
Atomic store resolution still enforces currentness and lifecycle fences.

The shared Rust hosting module supplies bounded strict JSON, Problem Details,
route normalization, correlation/W3C context and HTTP duration metrics.
Contract operations add versioned `flaggo.contract-service` spans, lifecycle
events, `flaggo.contract.operations`, `flaggo.contract.operation.duration`,
`flaggo.candidate.submissions` and `flaggo.activation.attempts`. Identifiers
belong on spans/logs, never metric dimensions. Standard OTLP/HTTP configuration
uses the existing observability adapter; no exporter is required for operation.
Typed response projections retain raw literal number tokens and preserve the
leading evidence-source `kind` discriminator without a production-wide
`serde_json` map-order or arbitrary-precision feature. Shutdown drains requests and
stops the worker before closing stores and flushing telemetry; Unix SIGTERM
handling is implemented but has not been live-tested on Windows.

## Verify

```powershell
cargo test --locked -p flaggo-contract-service
npm run test:tetris-full-loop
```

The opt-in live gate compares management behavior against actual unchanged
.NET hosts from an explicit pinned historical checkout over both SQLite producers,
checks idempotent admission, proves
differing .NET checked artifacts activate without rewriting, and evaluates the
activated artifacts in both runtimes. Comparison normalizes only mathematically
identical safe integral JSON number spelling and native invalid-body parser
diagnostic suffixes; statuses, Problem Details categories, identities, paging
cursors, persisted timestamps, ETags and projections are retained. The normal
tests also exercise real startup/restart and decoded OTLP exports.

The separate `npm run test:tetris-full-loop` gate builds and runs the
unchanged TypeScript clients against both Rust services and the existing
ingestion/materialization/analysis processes without building or launching
.NET. It verifies deterministic evidence analysis, independent draft review,
HTTP Candidate admission, automatic activation, SDK/REST parity, and exact
authority/provenance persistence after restart. See the
[full-loop instructions](../../examples/tetris-integration/README.md#rust-full-loop-acceptance).
The [combined migration acceptance command](../../tools/core-parity/README.md#opt-in-live-http-acceptance)
integrates both live HTTP comparison gates with library/store checks and
requires `--reference-root`; .NET is not a normal build or test prerequisite.

## Related documents

- [Contract clients and Contract Service](../../docs/design/architecture/CONTRACT_SERVICE.md)
- [Decision authority](../../docs/design/architecture/AUTHORITY.md)
- [Decision contract lifecycle](../../docs/design/contracts/LIFECYCLE.md)
- [Management API v3](../../contracts/openapi/flaggo-management-v3.yaml)
- [Internal service observability](../../docs/design/architecture/OBSERVABILITY.md)
