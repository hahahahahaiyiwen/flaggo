# Telemetry and Evidence Design

## Purpose

The telemetry/evidence component turns runtime observations into decision evidence. It is the bridge between application behavior and Flaggo's online runtime and async intelligence paths.

For the MVP, evidence can be simple and local. The design should still preserve a clean `IEvidenceProvider` seam so later implementations can use OpenTelemetry pipelines, metrics stores, or cloud data services.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Phase 1 wire-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md).

`ConfidenceReport` is defined in the shared contracts and is reused here so evidence, runtime responses, and proposals do not diverge.

## MVP responsibility

Evidence should provide:

- evidence snapshots/views for a decision definition,
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
  definition?: DecisionDefinitionRef;
  evidenceView: EvidenceViewRef;
  resolutionChain: DecisionTargetRef[];
  requiredEvidence?: EvidenceRequirement[];
  now: string;
};
```

## Evidence snapshot

```ts
type EvidenceSnapshot = {
  evidenceView: EvidenceViewRef;
  capturedAt: string;
  freshnessSeconds?: number;
  sampleSize?: number;
  confidence?: ConfidenceReport;
  metrics: Record<string, number>;
  quality: "missing" | "insufficient" | "sufficient" | "stale" | "conflicting";
};
```

## Runtime behavior

```text
Decision API
  -> resolves runtime target, control target, and evidence views
  -> requests evidence snapshots
  -> passes evidence to strategy executor and policy evaluator
  -> records evidence summary in audit
```

Missing evidence should not crash runtime. It should produce `quality = "missing"` or `quality = "insufficient"` and allow policy to decide whether fallback is required.

## Reuse across definition changes

Different decision definitions should not share active decision state by default, but they can reuse telemetry and evidence when the meaning is stable. This reduces cold start without letting a strategy trained for one definition control another definition.

| Layer | Reuse rule |
| --- | --- |
| Raw observations | Reusable across definitions when application, signal key, and target semantics match. |
| Evidence views | Reusable when signal key, target, window, and filters match. |
| Decision state/strategy | Not reusable by default; keyed by decision definition and control/runtime target. |

Example: a newly approved opaque revision of `tetris.dropInterval` may add `tetris.recoveryFailures`. It can reuse historical `tetris.boardPressure` and `tetris.recentPlacementTimeMs` observations because those immutable signal keys did not change. The new `tetris.recoveryFailures` signal starts cold unless historical observations already contain it.

Evidence view identity should include:

- immutable signal key,
- target,
- time window,
- filters.

If only some required evidence is warm, the evidence provider should surface that as partial quality rather than pretending the new contract is fully ready. Policy can then choose fallback, safe baseline, or limited activation.

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

## Declared metrics and inference inputs

Values used by online inference should be declared metrics first, then selected as inference inputs when the decision definition needs them on the hot path.

| Concept | Meaning | Example |
| --- | --- | --- |
| Declared metric | App-computed or pre-materialized signal with stable semantics. | `boardPressure` |
| Inference input | Declared metric supplied separately from ordinary context in the decision request. | `inputs["tetris.boardPressure"]` |
| Decision-record input | Inference input value captured when Flaggo returns a decision. | `decision.boardPressure` |
| Exposure-captured input | Inference input value copied after the client confirms the value was applied or rendered. | `exposure.boardPressure` |

Emitted metrics support broad async learning, including windows where no decision was requested. Decision-record inputs support audit of returned values. Exposure-captured inputs support decision-outcome attribution for the exact context in which the application actually applied or rendered a value.

The Decision API stores decision-time inputs and copies them into the exposure record when confirmation succeeds. Clients must not resubmit those inputs during confirmation. Later outcome telemetry correlates to `exposureId`; correlating only to a returned-but-unused `decisionId` would bias learning.

Design rule:

> If a value should influence online inference, declare it as a metric and select it as an inference input. The application should send the pre-aggregated current value; the service should not aggregate it on the hot path.

Phase 1 does not define a Flaggo-specific telemetry HTTP API. SDK telemetry should use OTLP; any direct/demo ingestion path is non-blocking and must not alter decision or exposure contracts.

## Tetris MVP evidence

Useful metrics:

- `hardDropRate`,
- `averagePlacementTimeMs`,
- `earlyGameOverRate`,
- `recoveryFailureRate`.

Live runtime values such as `boardPressure`, `recentPlacementTimeMs`, and `recoveryFailures` can come from runtime context only after they are declared as metrics and selected as inference inputs. The same metrics can be emitted over time, captured in decision records when Flaggo returns a decision, and copied into exposure records only after the client confirms application/rendering. Aggregated evidence can provide broader confidence and sample-size context.

## MVP non-goals

- Full telemetry warehouse.
- Complex metric query language.
- Long-term retention.
- High-cardinality optimization.
- Cross-service trace analytics.

Those belong to later telemetry/evidence iterations.
