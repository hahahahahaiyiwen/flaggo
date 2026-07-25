# Telemetry and Evidence Design

## Purpose

The telemetry/evidence component turns runtime observations into decision evidence. It is the bridge between application behavior and Flaggo's online runtime and async intelligence paths.

For the MVP, evidence can be simple and local. The design should still preserve a clean `IEvidenceProvider` seam so later implementations can use OpenTelemetry pipelines, metrics stores, or cloud data services.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

Evidence should provide:

- a scoped evidence snapshot for a decision surface,
- freshness and quality status,
- sample size when available,
- confidence when available,
- numeric metrics used by policy or strategy execution.

The MVP may use fixture or in-memory evidence. Runtime context can carry the most important live facts for Tetris.

## Core port

```ts
interface IEvidenceProvider {
  getSnapshot(input: EvidenceRequest): Promise<EvidenceSnapshot>;
}

type EvidenceRequest = {
  surface: string;
  scope: ScopeRef;
  resolutionChain: ScopeRef[];
  requiredEvidence?: EvidenceRequirement[];
  now: string;
};
```

## Evidence snapshot

```ts
type EvidenceSnapshot = {
  surface: string;
  scope: ScopeRef;
  capturedAt: string;
  freshnessSeconds?: number;
  sampleSize?: number;
  confidence?: number;
  metrics: Record<string, number>;
  quality: "missing" | "insufficient" | "sufficient" | "stale" | "conflicting";
};
```

## Runtime behavior

```text
Decision API
  -> resolves scope chain
  -> requests evidence snapshot
  -> passes evidence to strategy executor and policy evaluator
  -> records evidence summary in audit
```

Missing evidence should not crash runtime. It should produce `quality = "missing"` or `quality = "insufficient"` and allow policy to decide whether fallback is required.

## OpenTelemetry relationship

OpenTelemetry should be the preferred telemetry transport, but the MVP evidence provider does not need production-grade aggregation.

Initial options:

| Mode | Behavior |
| --- | --- |
| Fixture | Hard-coded or file-backed evidence snapshots for tests and demo. |
| In-memory | SDK/demo emits events into a local process or simple store. |
| OTel-compatible | SDK emits OTel-shaped telemetry; aggregation can be added later. |

Design rule:

> Evidence interfaces should not depend on a specific telemetry vendor or cloud provider.

## Tetris MVP evidence

Useful metrics:

- `hardDropRate`,
- `averagePlacementTimeMs`,
- `earlyGameOverRate`,
- `recoveryFailureRate`.

Live runtime facts such as `boardPressure`, `recentPlacementTimeMs`, and `recoveryFailures` can come from `runtimeContext`. Aggregated evidence can provide broader confidence and sample-size context.

## MVP non-goals

- Full telemetry warehouse.
- Complex metric query language.
- Long-term retention.
- High-cardinality optimization.
- Cross-service trace analytics.

Those belong to later telemetry/evidence iterations.
