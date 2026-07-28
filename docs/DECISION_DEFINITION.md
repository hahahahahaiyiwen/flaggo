# Decision Definition

## Purpose

A decision definition is the versioned contract for a Flaggo decision. It declares what may be decided, which target levels matter, which evidence may be used, what "better" means, and which safety/fallback rules constrain the system.

It is one of Flaggo's three top-level mental-model components:

```text
Decision Definition
Decision Evidence
Decision Intelligence
```

## Definition

A decision definition belongs to a stable decision key.

```text
decision key:
  tetris.dropInterval

decision definitions:
  tetris.dropInterval@1
  tetris.dropInterval@2
```

The key identifies the decision family. The definition revision identifies one immutable semantic version of that decision.

## What a decision definition owns

| Part | Meaning | Tetris example |
| --- | --- | --- |
| Decision key | Stable application-facing decision family. | `tetris.dropInterval` |
| Revision | Immutable semantic version. | `2` |
| Signals | Target hierarchy plus event schemas, app-emitted metrics, and service-aggregated metrics this decision may use for learning, validation, and online inference. | `session -> user -> cohort -> global`, `piece_placed`, `boardPressure`, `earlyLossRate` |
| Intent | Typed objective: natural-language product direction or metric-driven optimization over declared signals. | natural-language: challenging but playable; metric-objective: minimize early loss |
| Inference | Runtime inference target, app-emitted metric inputs, and fallback order. | target `session`, inputs `boardPressure`, fallback `cohort -> global` |
| Output contract | Result type, bounds, allowed values, step, default. | number, `200..1500`, step `50`, default `800` |
| Safety/policy/guardrails | Hard constraints, operating limits, and requested approval mode. Deployment/environment policy decides whether requested automatic approval is allowed. | gradual, cooldown, min evidence quality, max model uncertainty, requested automatic approval |
| Runtime context schema | Request-time facts the application must or may provide, including fields that identify target levels. | `sessionId`, `cohort`, board pressure |

## What it does not own

A decision definition does not own:

- raw telemetry history,
- evidence snapshots,
- app/build provenance,
- active strategy or governed state,
- rollout state,
- concrete runtime decision results,
- audit records.

Those belong to [Decision Evidence](DECISION_EVIDENCE.md), [Decision Intelligence](DECISION_INTELLIGENCE.md), governed state, and audit/explanation components.

## SDK-facing shape

The simple code-first UX should hide most contract machinery:

```ts
const dropInterval = flaggo.tune.number("tetris.dropInterval", {
  definition: {
    signals: {
      targetHierarchy: ["session", "user", "cohort", "global"],
      definitions: {
        boardPressure: {
          kind: "metric",
          type: "number",
          source: "app-emitted",
          range: [0, 1]
        },
        recentPlacementTimeMs: {
          kind: "metric",
          type: "number",
          source: "app-emitted"
        },
        recoveryFailures: {
          kind: "metric",
          type: "number",
          source: "app-emitted"
        },
        currentLevel: {
          kind: "metric",
          type: "number",
          source: "app-emitted"
        },
        piecePlaced: {
          kind: "event",
          emitAs: "piece_placed",
          fields: {
            placementTimeMs: "number",
            hardDrop: "boolean"
          }
        },
        sessionEnded: {
          kind: "event",
          emitAs: "session_ended",
          fields: {
            endReason: "string",
            durationSeconds: "number"
          }
        },
        earlyLossRate: {
          kind: "metric",
          type: "number",
          source: "service-aggregated",
          from: "sessionEnded.endReason",
          aggregation: "rate(endReason == 'early_loss')"
        },
        hardDropRate: {
          kind: "metric",
          type: "number",
          source: "service-aggregated",
          from: "piecePlaced.hardDrop",
          aggregation: "rate(hardDrop == true)"
        }
      }
    },
    intent: {
      type: "metric-objective",
      primary: { signal: "earlyLossRate", direction: "minimize" },
      secondary: [
        { signal: "hardDropRate", direction: "target", target: 0.45 },
        { signal: "recentPlacementTimeMs", direction: "minimize" }
      ],
      rationale: "Keep gameplay challenging but playable while reducing early frustration."
    },
    inference: {
      target: "session",
      inputs: ["boardPressure", "recentPlacementTimeMs", "recoveryFailures", "currentLevel"],
      fallbackOrder: ["cohort", "global"]
    },
    output: {
      default: 800,
      range: [200, 1500],
      step: 50
    },
    safety: "gradual",
    requestedApproval: "automatic",
    context: {
      sessionId: { type: "string", target: "session" },
      userId: { type: "string", target: "user" },
      cohort: { type: "string", target: "cohort" },
      currentLevel: "number",
      deviceType: "string",
      boardPressure: "number",
      recentPlacementTimeMs: "number",
      recoveryFailures: "number"
    }
  },
  context: {
    sessionId,
    userId,
    cohort: playerCohort,
    currentLevel: game.level,
    deviceType: device.type,
    boardPressure,
    recentPlacementTimeMs,
    recoveryFailures
  }
});
```

The `definition` block compiles to a versioned decision definition. Runtime values in `context` are used for online inference only when their signal names are listed in `inference.inputs`. The `definition.context` schema tells Flaggo which fields are expected and which fields identify target hierarchy levels. The developer names the stable decision key; Flaggo tooling and the registry manage semantic revisions.

Automatic approval requires executable objectives. If a definition requests `requestedApproval: "automatic"`, the definition should use `intent.type: "metric-objective"` and typed policy constraints; natural-language-only intent should require human approval or policy-default handling.

Signals are the single declaration surface for facts Flaggo may understand:

| Concept | Source | Used by | Example |
| --- | --- | --- | --- |
| Declared event | Domain event emitted over time. | Async learning, evidence views, validation, audit. | `piece_placed.placementTimeMs` |
| App-emitted metric | Application-computed metric with stable semantics. | Async learning and, if selected, online inference. | `boardPressure` |
| Service-aggregated metric | Metric derived by Flaggo from declared events or metrics. | Async learning, evidence views, validation, policy. | `earlyLossRate` |
| Inference input | Declared app-emitted metric supplied with the decision request. | Online inference and strategy execution. | `context.boardPressure` |
| Exposure-captured input | Inference input value captured with the decision record. | Later learning and outcome correlation. | `exposure.boardPressure` when `850ms` was returned |

If online inference should branch on a value, it must be declared as an app-emitted metric and selected as `inference.inputs`. The application should provide the pre-aggregated value with the request; the online service should not aggregate it on the hot path. Service-aggregated metrics must declare their source signal and aggregation expression. Evidence views can be derived internally from the definition revision, signal declarations, target hierarchy, and requested windows. Exposure capture is still useful because it records the exact input values present when a decision was rendered.

Intent is typed so Flaggo can distinguish product guidance from measurable objectives:

```ts
intent: {
  type: "natural-language",
  text: "challenging-but-playable"
}
```

```ts
intent: {
  type: "metric-objective",
  primary: { signal: "earlyLossRate", direction: "minimize" },
  secondary: [
    { signal: "hardDropRate", direction: "target", target: 0.45 },
    { signal: "recentPlacementTimeMs", direction: "minimize" }
  ],
  rationale: "Keep the game challenging while reducing early frustration."
}
```

Natural-language intent is useful for early product design and human review. Metric-objective intent is useful when the team wants async intelligence to optimize against declared signals with explicit directions and targets.

## Target hierarchy

The target hierarchy is the abstraction that prevents separate hard-coded models for async learning, governed state, online inference, evidence, policy, and fallback.

```text
session -> user -> cohort -> global
```

Each path resolves a target role from the same hierarchy:

| Target role | Chosen by | Meaning |
| --- | --- | --- |
| Runtime target | Online inference | Concrete entity receiving the decision now. |
| Learning target | Async intelligence | Population or slice with enough evidence for analysis. |
| Control target | Governance | Boundary where approved state is stored. |
| Evidence target | Evidence layer | Aggregation boundary for an evidence view. |
| Fallback target | Runtime/governance | Broader safe level used when specific resolution fails. |

Example:

```text
runtime target:
  session:game-456

learning target:
  cohort:new_players

control target:
  cohort:new_players

fallback target:
  global
```

## Versioning rule

Semantic changes create a new definition revision. Examples:

- output type or bounds change,
- `signals.targetHierarchy` changes,
- `inference.target` changes,
- `inference.fallbackOrder` changes,
- signal semantics change,
- optimization intent changes,
- safety/policy envelope changes.

Metadata-only changes may keep the same semantic revision if the registry can prove runtime behavior is unchanged.

## Tetris example

```text
DecisionDefinition
  key: tetris.dropInterval
  revision: 2
  signals:
    targetHierarchy: session -> user -> cohort -> global
    definitions:
      piecePlaced:
        kind: event
      boardPressure:
        kind: metric
        source: app-emitted
      earlyLossRate:
        kind: metric
        source: service-aggregated
  intent:
    type: natural-language
    text: challenging-but-playable
  inference:
    target: session
    inputs:
      - boardPressure
      - recentPlacementTimeMs
      - currentLevel
    fallbackOrder: cohort -> global
  output:
    type: number
    range: 200..1500
    step: 50
    default: 800
  requestedApproval: automatic
  safety:
    gradual
```

`requestedApproval` is part of the definition contract, but it is only a request. Deployment or environment policy decides whether automatic approval is actually permitted for the target, risk level, and policy envelope.

## Design rule

> A decision definition declares the semantic contract. It constrains decision intelligence, but it does not contain learned authority or runtime results.
