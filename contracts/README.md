# Phase 1 Executable Contracts

This directory is the executable projection of the accepted Phase 1 design in
[`docs/design/API_CONTRACT_PROPOSAL.md`](../docs/design/API_CONTRACT_PROPOSAL.md).
The OpenAPI documents and JSON Schemas are the accepted Phase 1 authority for
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

The fixture manifest indexes 69 cases covering all 43 required scenarios from
the proposal. SDK and service implementations must use the same artifacts
rather than maintain independent wire DTOs.

## Validate

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

Validation checks all schemas, OpenAPI structure and local references, fixture
manifest coverage, positive request/response bodies, negative schema cases,
semantic value contracts, and normalized per-definition and bundle digests.
It does not start network services or access remote schema registries.

## Run the mock

```powershell
python contracts\mock\fixture-server\server.py --port 8080
```

See the [fixture server README](mock/fixture-server/README.md) for request
selection and discovery endpoints.

## Change policy

Contract changes must update the affected schema, OpenAPI operation, fixtures,
manifest, proposal compatibility notes, and conformance checks together. The
accepted Phase 1 baseline changes only through an explicit compatible revision
or a documented breaking-version decision.
