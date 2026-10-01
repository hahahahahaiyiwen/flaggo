# Executable Contracts

This directory is the executable projection of the service-owned APIs and
telemetry contracts. The OpenAPI documents and JSON Schemas are the authority
for Flaggo-owned wire and event behavior. The OTLP profile selects required
capabilities from the upstream OpenTelemetry Protocol without redefining its
payload messages. The conformance gate must remain green for every change.

## Layout

```text
contracts/
  openapi/       Runtime and management OpenAPI 3.1 documents
  schemas/       Draft 2020-12 JSON Schemas
  fixtures/      Golden HTTP, SDK-local, and schema-negative cases
  otel/          Flaggo telemetry schema and OTLP/HTTP capability profile
  conformance/   Fixture manifest and offline validation
  mock/          Fixture-backed development server
```

The fixture manifest indexes the current conformance scenarios. SDK and service
implementations use these artifacts as the shared wire authority.

The current management contract is
[`flaggo-management-v3.yaml`](openapi/flaggo-management-v3.yaml). A decision
name identifies a versioned `DecisionContract` resource, while
`contractDigest` identifies one immutable accepted version. The API exposes
dry-run validation, idempotent create-or-update by name, current-version lookup,
and cursor-paginated historical-version lookup. A newly created version is
returned only after its generated default executable is active. The API has no
bundle, numeric server revision, compatibility classification, or manual
bundle-approval resource. Unauthenticated liveness and readiness probes use the
shared health models from `runtime-models-v3.schema.json`.

[`management-models-v3.schema.json`](schemas/management-models-v3.schema.json)
contains the strict `DecisionContract`, `DecisionExecutable`, validation-result,
immutable-version, and version-list shapes. Description-only changes remain
non-semantic and resolve to the existing digest instead of creating a new
version. Its JSON Schema and CEL value profiles remain intentionally bounded by
the deferred design work recorded in
[`CONTRACTS.md`](../docs/design/contracts/CONTRACTS.md).

Errors use the RFC 9457 members from
[`problem-details-v3.schema.json`](schemas/problem-details-v3.schema.json)
and may include problem-type extension members. Flaggo defines no shared custom
body fields: problem identity is the `type` URI, while request correlation and
retry timing remain HTTP headers.

The runtime contract is
[`flaggo-runtime-v3.yaml`](openapi/flaggo-runtime-v3.yaml). Its main operation
posts a complete `RuntimeInput` to the exact immutable contract-version
resource:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

The route never resolves the named resource's current version. Every successful
response identifies the exact contract and executable digests. The obsolete v2
definition-bundle contract and its consumers have been removed.

## Telemetry contracts

[`telemetry-events-v1.schema.json`](schemas/telemetry-events-v1.schema.json)
defines the strict logical projection of Flaggo-owned OpenTelemetry log events.
It currently covers `flaggo.decision.received` and the optional
`flaggo.outcome.observed` helper event. The schema validates event names,
attributes, evaluation provenance, identities, and value representations
without duplicating the surrounding OTLP envelope.

The source for the immutable OpenTelemetry Schema File is
[`flaggo-telemetry-schema-1.0.0.yaml`](otel/flaggo-telemetry-schema-1.0.0.yaml).
Its reserved publication URL is:

```text
https://flaggo.dev/schemas/telemetry/1.0.0
```

The SDK must leave its instrumentation-scope `schemaUrl` unset until that URL
is published and retrievable. Publishing the file and then enabling SDK
emission are deployment work, not part of this contract-only change.

[`flaggo-otlp-http-profile-v1.json`](otel/flaggo-otlp-http-profile-v1.json)
selects the Phase 4 OTLP/HTTP surface: logs, metrics, and traces; Protobuf and
Protobuf JSON encodings; identity and gzip compression; the 64 MiB decompressed
request limit; full-request OTLP success after durable inbox enqueue; standard
OTLP failure responses; and no Phase 4 ingestion authentication. Upstream
`opentelemetry-proto` definitions remain the payload authority. Flaggo defines
no custom OTLP request model or OTLP OpenAPI operation.

The fixture surface contains logical event cases plus OTLP/HTTP JSON and
gzip-compressed Protobuf exchanges. The offline gate validates the profile,
schema files, fixture/profile agreement, strict event JSON, event hashes, and
the binary export envelope. Real-host OTLP profile dispatch remains a separate
implementation conformance check.

## Validate

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

Validation checks registered schemas, OpenAPI structure and local references,
fixture-manifest coverage, positive request/response bodies, negative schema
cases, semantic value contracts, Flaggo event semantics, the OTLP/HTTP profile,
the v3 DecisionContract management surface, and the v3 runtime evaluation
surface. The Tetris consumer and host harness use the v3 management and runtime
APIs; the deleted v2 bundle schema is no longer part of their validation path.
It does not start network services or access remote schema registries.

The `Contracts` GitHub Actions workflow runs the local Tetris and Adaptive
Worker real-host integrations in dedicated jobs rather than folding them into
the workspace `npm test` gate.

## Run the fixture server

```powershell
python contracts\mock\fixture-server\server.py --port 8080
```

This server is a conformance tool, not Contract Service or Decision Service.
See the [fixture server README](mock/fixture-server/README.md) for request
selection and discovery endpoints.

## Change policy

Contract changes must update the affected schema, OpenAPI operation, fixtures,
manifest, proposal compatibility notes, and conformance checks together. The
baseline changes only through an accepted contract revision with aligned
consumers. This repository intentionally provides no obsolete-format adapter.
