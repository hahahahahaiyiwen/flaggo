# Phase 1 Executable Contracts

This directory is the executable projection of the accepted Phase 1 design in
[`docs/design/API_CONTRACT_PROPOSAL.md`](../docs/design/API_CONTRACT_PROPOSAL.md).
The OpenAPI documents and JSON Schemas become authoritative for wire behavior
only after all validation checks pass and the contract change is merged.

## Layout

```text
contracts/
  openapi/       Runtime and management OpenAPI 3.1 documents
  schemas/       Draft 2020-12 JSON Schemas
  fixtures/      Golden HTTP, SDK-local, and schema-negative cases
  conformance/   Fixture manifest and offline validation
  mock/          Fixture-backed development server
```

The fixture manifest indexes 65 cases covering all 43 required scenarios from
the proposal. SDK and service implementations must use the same artifacts
rather than maintain independent wire DTOs.

## Validate

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python contracts\conformance\validate.py
```

Validation checks all schemas, OpenAPI structure and local references, fixture
manifest coverage, positive request/response bodies, and negative schema cases.
It does not start network services or access remote schema registries.

## Run the mock

```powershell
python contracts\mock\fixture-server\server.py --port 8080
```

See the [fixture server README](mock/fixture-server/README.md) for request
selection and discovery endpoints.

## Change policy

Contract changes must update the affected schema, OpenAPI operation, fixtures,
manifest, proposal compatibility notes, and conformance checks together. Phase
1 is not frozen merely because these files exist; it freezes only after the
artifact set is reviewed, validated, and merged.
