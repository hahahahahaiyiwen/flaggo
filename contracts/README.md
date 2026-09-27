# Executable Contracts

This directory is the executable projection of the service-owned API designs:
[Contract Service](../docs/design/architecture/CONTRACT_SERVICE.md),
[Decision Service](../docs/design/architecture/RUNTIME.md), and
[Evidence](../docs/design/architecture/EVIDENCE.md). The OpenAPI documents and
JSON Schemas are the authority for current wire behavior. Their conformance
gate must remain green for every change.

## Layout

```text
contracts/
  openapi/       Runtime and management OpenAPI 3.1 documents
  schemas/       Draft 2020-12 JSON Schemas
  fixtures/      Golden HTTP, SDK-local, and schema-negative cases
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
bundle-approval resource.

[`management-models-v3.schema.json`](schemas/management-models-v3.schema.json)
contains the strict `DecisionContract`, `DecisionExecutable`, validation-result,
immutable-version, and version-list shapes. Description-only changes remain
non-semantic and resolve to the existing digest instead of creating a new
version. Its JSON Schema and CEL value profiles remain intentionally bounded by
the deferred design work recorded in
[`CONTRACTS.md`](../docs/design/contracts/CONTRACTS.md).

Errors use the RFC 9457 members from
[`problem-details-v3.schema.json`](schemas/problem-details-v3.schema.json)
without Flaggo-specific body fields. Problem identity is the `type` URI;
request correlation and retry timing remain HTTP headers.

The runtime contract is
[`flaggo-runtime-v3.yaml`](openapi/flaggo-runtime-v3.yaml). Its main operation
posts a complete `RuntimeInput` to the exact immutable contract-version
resource:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

The route never resolves the named resource's current version. Every successful
response identifies the exact contract and executable digests. The obsolete v2 definition-bundle contract and its consumers have been removed.

The Phase 3 runtime fixture surface is JSON-only. It covers runtime decisions
and health; there are no executable Evidence or OTLP routes.

## Validate

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

Validation checks registered schemas, OpenAPI structure and local references,
fixture-manifest coverage, positive request/response bodies, negative schema
cases, semantic value contracts, the v3 DecisionContract management surface,
and the v3 runtime evaluation surface. The Tetris consumer and host harness use
the v3 management and runtime APIs; the deleted v2 bundle schema is no longer
part of their validation path.
It does not start network services or access remote schema registries.

The `Contracts` GitHub Actions workflow runs the local Tetris host harness in a
separate `tetris-integration` job rather than folding it into the workspace
`npm test` gate. Worker and stock-Collector integrations have their own jobs.

## Run the mock

```powershell
python contracts\mock\fixture-server\server.py --port 8080
```

See the [fixture server README](mock/fixture-server/README.md) for request
selection and discovery endpoints.

## Change policy

Contract changes must update the affected schema, OpenAPI operation, fixtures,
manifest, proposal compatibility notes, and conformance checks together. The
baseline changes only through an accepted contract revision with aligned
consumers. This repository intentionally provides no obsolete-format adapter.
