# Executable Contracts

This directory is the executable projection of the accepted API design in
[`docs/design/API_CONTRACT_PROPOSAL.md`](../docs/design/API_CONTRACT_PROPOSAL.md).
The OpenAPI documents and JSON Schemas are the authority for
wire behavior. Their conformance gate must remain green for every change.

## Layout

```text
contracts/
  openapi/       Runtime and management OpenAPI 3.1 documents
  schemas/       Draft 2020-12 JSON Schemas
  fixtures/      Golden HTTP, SDK-local, and schema-negative cases
  conformance/   Fixture manifest and offline validation
  mock/          Fixture-backed development server
```

The fixture manifest indexes 75 cases covering all 43 required scenarios from
the proposal. SDK and service implementations must use the same artifacts
rather than maintain independent wire DTOs.

The current breaking replacement is manifest
`flaggo.decision-definition-bundle/v2`: one result/default, typed request or
evidence inputs, and decision-local native OTel bindings. Producer schemas,
inline declarations/extraction, old input arrays, and v1 manifests are removed.
Deterministic rules permit null confidence. Current publication returns an
approved-definition receipt; initial-authority activation/readiness is #40.

Three native scope-denial fixtures use `protocol: "otlp-protobuf"` and
`protobuf` message projections rather than JSON wire bodies. The exhaustive
.NET fixture runner encodes generated OTLP requests and compares binary
`google.rpc.Status` responses. Python verifies route/media/status/scope
coverage; the JSON mock does not advertise these native fixtures.

Approval-required change sets contain at least one newly `created` definition
or `semantic-change`; metadata-only changes remain immediately applicable and
cannot masquerade as an approval request.

## Validate

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

Validation checks all schemas, OpenAPI structure and local references, fixture
manifest coverage, positive request/response bodies, negative schema cases,
semantic value contracts, and normalized per-definition and bundle digests.
It also validates the canonical Tetris definition artifact directly against
the frozen Draft 2020-12 bundle schema and proves additional and unevaluated
properties are rejected.
The SDK artifact test pins both the full canonical bundle digest and the
per-definition semantic digest. Intentional artifact edits must pass this
frozen-schema gate first, then update both SDK-computed pins in the same
reviewed change.
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
