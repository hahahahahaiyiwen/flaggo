# Flaggo Component Design Index

This folder contains focused design documents for Flaggo system components.

Top-level product framing remains in:

- [Manifesto](../MANIFESTO.md)
- [Hero Scenario](../HERO_SCENARIO.md)
- [High-Level Design](../DESIGN.md)
- [Mental Model](../MENTAL_MODEL.md)
- [Decision Definition](../DECISION_DEFINITION.md)
- [Decision Evidence](../DECISION_EVIDENCE.md)
- [Decision Intelligence](../DECISION_INTELLIGENCE.md)
- [MVP Implementation Guide](../IMPLEMENTATION_GUIDE.md)
- [Phase 1 API Contract Proposal](API_CONTRACT_PROPOSAL.md)
- [Shared Contracts](shared-contracts/README.md)

## Component folders

Each component under [`DESIGN.md` / System components](../DESIGN.md#system-components) has exactly one folder:

| # | High-level component | Design folder | Purpose |
|---|---|---|---|
| 1 | Client library | [client-library](client-library/README.md) | Developer-facing SDK for telemetry, decision declarations, runtime context, and decision calls. |
| 2 | Decision API service | [decision-api](decision-api/README.md) | Runtime service API that evaluates definitions/evidence and returns `RuntimeDecisionResult` values. |
| 3 | Telemetry and evidence service | [telemetry-evidence](telemetry-evidence/README.md) | Telemetry ingestion, evidence aggregation, OpenTelemetry integration, and evidence snapshots. |
| 4 | Contract and registry service | [contract-registry](contract-registry/README.md) | Versioned storage for decision definitions, output contracts, signal declarations, inference configuration, policies, and fallbacks. |
| 5 | Policy service | [policy](policy/README.md) | Deterministic safety gate for constraints, confidence, cooldowns, approvals, and guardrails. |
| 6 | State service | [state](state/README.md) | Active decision values, previous decisions, cooldowns, rollouts, overrides, and rollback transition metadata. |
| 7 | Decision reasoning engine | [reasoning-engine](reasoning-engine/README.md) | Candidate decision generation using rules, statistics, bandits, AI, or hybrid methods. |
| 8 | Audit and explanation service | [audit-explanation](audit-explanation/README.md) | Decision audit records, evidence lineage, policy outcomes, and explanations. |
| 9 | Operator console | [operator-console](operator-console/README.md) | Human governance surface for inspection, control, override, pause, approval, and rollback. |

## Cross-cutting contracts

[shared-contracts](shared-contracts/README.md) contains provider-neutral interfaces shared by the nine components. It is a cross-cutting contract package, not an additional system component.

The [Phase 1 API Contract Proposal](API_CONTRACT_PROPOSAL.md) maps those domain contracts to the runtime, exposure, management, and health wire boundaries. Its product decisions are accepted; the wire contract remains a draft until validated OpenAPI, JSON Schema, fixtures, and conformance tests encode them.
