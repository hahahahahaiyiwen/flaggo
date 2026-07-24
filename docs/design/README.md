# Flaggo Component Design Index

This folder contains focused design documents for Flaggo system components.

Top-level product framing remains in:

- [Manifesto](..\MANIFESTO.md)
- [Hero Scenario](..\HERO_SCENARIO.md)
- [High-Level Design](..\DESIGN.md)
- [Decision Surface](..\DECISION_SURFACE.md)
- [Decision Scope](..\DECISION_SCOPE.md)
- [Decision Factors](..\DECISION_FACTORS.md)

## Component folders

| Component | Purpose |
|---|---|
| [client-library](client-library\README.md) | Developer-facing SDK for telemetry, decision declarations, runtime context, and decision calls. |
| [decision-api](decision-api\README.md) | Runtime service API that evaluates scoped decisions and returns governed results. |
| [telemetry-evidence](telemetry-evidence\README.md) | Telemetry ingestion, evidence aggregation, OpenTelemetry integration, and evidence snapshots. |
| [contract-registry](contract-registry\README.md) | Versioned storage for decision surfaces, action spaces, goals, policies, scopes, and fallbacks. |
| [policy](policy\README.md) | Deterministic safety gate for constraints, confidence, cooldowns, approvals, and guardrails. |
| [state](state\README.md) | Active decision values, previous decisions, cooldowns, rollouts, overrides, and rollback state. |
| [reasoning-engine](reasoning-engine\README.md) | Candidate decision generation using rules, statistics, bandits, AI, or hybrid methods. |
| [audit-explanation](audit-explanation\README.md) | Decision audit records, evidence lineage, policy outcomes, and explanations. |
| [operator-console](operator-console\README.md) | Human governance surface for inspection, control, override, pause, approval, and rollback. |

