# Flaggo component design index

This folder contains focused designs for Flaggo system components and exact
contract boundaries.

Canonical context:

- [Documentation home](../README.md)
- [Manifesto](../MANIFESTO.md)
- [Architecture overview](../architecture/OVERVIEW.md)
- [Decision definition](../architecture/DECISION_DEFINITION.md)
- [Evidence](../architecture/EVIDENCE.md)
- [Authority](../architecture/AUTHORITY.md)
- [Runtime execution](../architecture/RUNTIME_EXECUTION.md)
- [Tetris scenario](../scenarios/TETRIS.md)
- [Project roadmap](https://github.com/users/hahahahahaiyiwen/projects/3)
- [Phase 1 API contract proposal](API_CONTRACT_PROPOSAL.md)
- [Phase 3 Tetris integration](tetris-integration/README.md)
- [Shared contracts](shared-contracts/README.md)

## Component folders

Each component in the
[architecture overview](../architecture/OVERVIEW.md#system-components) has
exactly one folder:

| # | High-level component | Design folder | Purpose |
|---|---|---|---|
| 1 | Client library | [client-library](client-library/README.md) | Developer-facing SDK for telemetry, decision declarations, runtime context, and decision calls. |
| 2 | Decision API service | [decision-api](decision-api/README.md) | Runtime service API that evaluates definitions, governed state, live inputs, policy, and optional evidence. |
| 3 | Telemetry and evidence service | [telemetry-evidence](telemetry-evidence/README.md) | Telemetry ingestion, evidence aggregation, OpenTelemetry integration, and evidence snapshots. |
| 4 | Contract and registry service | [contract-registry](contract-registry/README.md) | Versioned storage for decision definitions, output contracts, signal declarations, inference configuration, policies, and fallbacks. |
| 5 | Policy service | [policy](policy/README.md) | Deterministic safety gate for action-space, runtime, authority, and conditional evidence constraints. |
| 6 | State service | [state](state/README.md) | Active authority, state identity/generation, activation lineage, CAS, replay, and runtime projection. |
| 7 | Decision reasoning engine | [reasoning-engine](reasoning-engine/README.md) | Bounded runtime strategy execution plus optional future proposal generation. |
| 8 | Audit and explanation service | [audit-explanation](audit-explanation/README.md) | Decision audit records, evidence lineage, policy outcomes, and explanations. |
| 9 | Operator console | [operator-console](operator-console/README.md) | Current audit/state visibility seam and future human governance surface; override, pause, and rollback require later contracts. |

## Cross-cutting contracts

[shared-contracts](shared-contracts/README.md) contains provider-neutral interfaces shared by the nine components. It is a cross-cutting contract package, not an additional system component.

The [Phase 1 API Contract Proposal](API_CONTRACT_PROPOSAL.md) maps those domain contracts to the runtime, exposure, management, and health wire boundaries. Its accepted executable projection lives under [`contracts/`](../../contracts/README.md). Future wire changes follow the baseline's explicit compatibility and revision process.

The [Phase 3 Tetris Integration](tetris-integration/README.md) composes these
component boundaries into the cloud-free hero scenario using authenticated
bundle approval and the shared governed-state activation boundary without
moving management authority into the browser.
