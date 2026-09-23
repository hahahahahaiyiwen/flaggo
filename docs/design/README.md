# Flaggo design index

Detailed designs follow the logical server boundaries defined by the
[architecture overview](../architecture/OVERVIEW.md). A boundary is first-class
only when it owns a domain or external-system contract.

## External boundaries

| Boundary | Design |
| --- | --- |
| Client SDK | [Client library](client-library/README.md) |
| Application telemetry pipeline | Application-owned OpenTelemetry APIs, providers, exporters, and Collector configuration; see [OTel Ingestion](otel-ingestion/README.md) for the Flaggo ingress boundary. |

## Server services and workers

| Boundary | Responsibility | Design |
| --- | --- | --- |
| Contract Service | Definition lifecycle, approval, readiness, and authority activation orchestration. | [Contract Service](contract-service/README.md) |
| Decision Service | Online decision and exposure APIs, deterministic execution, constraints, fallback, and durable record append. | [Decision Service](decision-service/README.md) |
| OTel Ingestion | OTLP intake, binding validation, normalization, observation append, and confirmed-exposure outcome attribution. | [OTel Ingestion](otel-ingestion/README.md) |
| Async Analysis Pipeline | Offline candidate production using contracts, evidence, and current state. | [Async Analysis Pipeline](async-analysis/README.md) |

## Durable stores

| Boundary | Responsibility | Design |
| --- | --- | --- |
| Contract Store | Definitions, revisions, approvals, and readiness metadata. | Owned by [Contract Service](contract-service/README.md#contract-store) |
| State Store | Stable authority heads, immutable state, CAS, replay, and lineage. | [State Store](state-store/README.md) |
| Evidence Store | Observations, derived evidence, decisions, exposures, and outcomes. | [Evidence Store](evidence-store/README.md) |

## Embedded capabilities

These are important behaviors, but not independent server components:

- decision constraints are declared in definitions, validated by Contract
  Service, and evaluated by Decision Service;
- bounded numeric-rule execution runs inside Decision Service;
- durable decision recording is part of Decision Service success;
- explanation is a deterministic projection of stored decision facts; and
- an operator console is a future client of service/query APIs.

## Cross-cutting contracts

[Shared contracts](shared-contracts/README.md) owns provider-neutral data and
wire shapes. Module service and infrastructure ports stay beside their
consumers.

The [Phase 1 API contract proposal](API_CONTRACT_PROPOSAL.md) and executable
[`contracts/`](../../contracts/README.md) describe current wire behavior.

The [Tetris integration](tetris-integration/README.md) composes the boundaries
into the cloud-free hero scenario.
