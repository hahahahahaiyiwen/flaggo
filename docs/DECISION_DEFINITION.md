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
| Signal references | Explicit allow-list of externally defined typed signal handles this decision may use for learning, validation, guardrails, objectives, and online inference. | `gameplaySignals.boardPressure`, `gameplaySignals.earlyLossRate` |
| Intent | Typed objective: natural-language product direction or metric-driven optimization over declared signals. | natural-language: challenging but playable; metric-objective: minimize early loss |
| Inference | Runtime inference target, app-emitted metric inputs, and fallback order. | target `session`, inputs `boardPressure`, fallback `cohort -> global` |
| Output contract | Result type, bounds, allowed values, step, default. | number, `200..1500`, step `50`, default `800` |
| Safety/policy/guardrails | Hard constraints, operating limits, and requested approval mode. Deployment/environment policy decides whether requested automatic approval is allowed. | gradual, cooldown, min evidence quality, max model uncertainty, requested automatic approval |
| Runtime context schema | Request-time facts the application must or may provide, including fields that identify target levels. | `sessionId`, `cohort`, board pressure |

## What it does not own

A decision definition does not own:

- raw telemetry history,
- signal schemas,
- evidence snapshots,
- app/build provenance,
- active strategy or governed state,
- rollout state,
- concrete runtime decision results,
- audit records.

Those belong to [Decision Evidence](DECISION_EVIDENCE.md), [Decision Intelligence](DECISION_INTELLIGENCE.md), governed state, and audit/explanation components.

## Signal ownership

Signals are defined once near their producer. The signal key is the immutable semantic identity for schema, type, units, range, and meaning:

```ts
// gameplaySignals.ts
export const gameplaySignals = flaggo.signals.define({
  boardPressure: flaggo.metric.number({
    key: "tetris.boardPressure",
    range: [0, 1]
  }),

  recentPlacementTimeMs: flaggo.metric.number({
    key: "tetris.recentPlacementTimeMs",
    unit: "ms"
  }),

  recoveryFailures: flaggo.metric.number({
    key: "tetris.recoveryFailures"
  }),

  currentLevel: flaggo.metric.number({
    key: "tetris.currentLevel"
  }),

  piecePlaced: flaggo.event({
    key: "tetris.piecePlaced",
    fields: {
      placementTimeMs: flaggo.number({ unit: "ms" }),
      hardDrop: flaggo.boolean()
    }
  }),

  sessionEnded: flaggo.event({
    key: "tetris.sessionEnded",
    fields: {
      endReason: flaggo.string(),
      durationSeconds: flaggo.number({ unit: "s" })
    }
  }),

  earlyLossRate: flaggo.metric.derived({
    key: "tetris.earlyLossRate",
    type: "number",
    from: ["tetris.sessionEnded"],
    aggregation: "rate(endReason == 'early_loss')",
    window: "24h"
  }),

  hardDropRate: flaggo.metric.derived({
    key: "tetris.hardDropRate",
    type: "number",
    from: ["tetris.piecePlaced"],
    aggregation: "rate(hardDrop == true)",
    window: "24h"
  })
});
```

Emission imports and uses the typed handle. It should not redefine schemas inside every `emit(...)` call:

```ts
gameplaySignals.boardPressure.emit(boardPressure);

gameplaySignals.piecePlaced.emit({
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});
```

If a signal schema or meaning changes incompatibly, introduce a new key such as `tetris.boardPressurePercent`. An internal schema digest can detect conflicting definitions under the same key, but there is no separate public signal name or revision.

## SDK-facing shape

The simple code-first UX should hide most contract machinery:

```ts
const dropIntervalDecision = await flaggo.tune.number("tetris.dropInterval", {
  definition: {
    signals: {
      allow: [
        gameplaySignals.boardPressure,
        gameplaySignals.recentPlacementTimeMs,
        gameplaySignals.recoveryFailures,
        gameplaySignals.currentLevel,
        gameplaySignals.piecePlaced,
        gameplaySignals.sessionEnded,
        gameplaySignals.earlyLossRate,
        gameplaySignals.hardDropRate
      ]
    },
    targetHierarchy: ["session", "user", "cohort", "global"],
    intent: {
      type: "metric-objective",
      primary: { signal: gameplaySignals.earlyLossRate, direction: "minimize" },
      secondary: [
        { signal: gameplaySignals.hardDropRate, direction: "target", target: 0.45 },
        { signal: gameplaySignals.recentPlacementTimeMs, direction: "minimize" }
      ],
      rationale: "Keep gameplay challenging but playable while reducing early frustration."
    },
    inference: {
      target: "session",
      inputs: [
        gameplaySignals.boardPressure,
        gameplaySignals.recentPlacementTimeMs,
        gameplaySignals.recoveryFailures,
        gameplaySignals.currentLevel
      ],
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
      deviceType: "string"
    }
  },
  context: {
    sessionId,
    userId,
    cohort: playerCohort,
    deviceType: device.type,
    boardPressure: gameplaySignals.boardPressure.value(boardPressure),
    recentPlacementTimeMs: gameplaySignals.recentPlacementTimeMs.value(recentPlacementTimeMs),
    recoveryFailures: gameplaySignals.recoveryFailures.value(recoveryFailures),
    currentLevel: gameplaySignals.currentLevel.value(game.level)
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
await flaggo.exposures.confirm(dropIntervalDecision.decisionId);
```

The `definition` block compiles to a versioned decision definition. Runtime values in `context` are used for online inference only when supplied through typed signal values whose handles are listed in `inference.inputs`. The `definition.context` schema tells Flaggo which non-signal fields identify target hierarchy levels or runtime metadata. The developer names the stable decision key; Flaggo tooling and the registry manage semantic revisions. The `flaggo.tune.number(...)` surface returns a number decision receipt: application code applies `.value`, while `.decisionId` supports exposure confirmation.

Automatic approval requires executable objectives. If a definition requests `requestedApproval: "automatic"`, the definition should use `intent.type: "metric-objective"` and typed policy constraints; natural-language-only intent should require human approval or policy-default handling.

Signal handles are the single declaration surface for facts Flaggo may understand:

| Concept | Source | Used by | Example |
| --- | --- | --- | --- |
| Declared event | Domain event emitted through a typed signal handle. | Async learning, evidence views, validation, audit. | `gameplaySignals.piecePlaced` |
| App-emitted metric | Application-computed metric with stable semantics. | Async learning and, if selected, online inference. | `gameplaySignals.boardPressure` |
| Derived signal | Metric declared from other signal handles and an aggregation expression. | Async learning, evidence views, validation, policy. | `gameplaySignals.earlyLossRate` |
| Inference input | Allowed app-emitted metric supplied with the decision request through `.value(...)`. | Online inference and strategy execution. | `gameplaySignals.boardPressure.value(boardPressure)` |
| Decision-record input | Inference input value captured when a value is returned. | Auditing what Flaggo decided for the request. | `decision.boardPressure` when `850ms` was returned |
| Exposure-captured input | Inference input value copied to an exposure only after the client confirms the value was applied or rendered. | Later learning and outcome correlation. | `exposure.boardPressure` after `confirmExposure(decisionId)` |

If online inference should branch on a value, it must be declared once as an app-emitted metric handle and selected as `inference.inputs`. The application should provide the pre-aggregated value with the request through that handle; the online service should not aggregate it on the hot path. Aggregated metrics must be declared as derived signal handles with their source signals and aggregation expression. Evidence views can be derived internally from the definition revision, referenced signal definitions, target hierarchy, and requested windows. Decision records capture returned values; exposure capture is still useful because it records the exact input values present when the application actually applied or rendered a decision.

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
- `targetHierarchy` changes,
- `inference.target` changes,
- `inference.fallbackOrder` changes,
- allowed signal roles change,
- optimization intent changes,
- safety/policy envelope changes.

Metadata-only changes may keep the same semantic revision if the registry can prove runtime behavior is unchanged.

## Tetris example

```text
DecisionDefinition
  key: tetris.dropInterval
  revision: 2
  targetHierarchy: session -> user -> cohort -> global
  signals:
    allow:
      - tetris.piecePlaced
      - tetris.sessionEnded
      - tetris.boardPressure
      - tetris.recentPlacementTimeMs
      - tetris.earlyLossRate
      - tetris.hardDropRate
  intent:
    type: metric-objective
    primary:
      signal: tetris.earlyLossRate
      direction: minimize
  inference:
    target: session
    inputs:
      - tetris.boardPressure
      - tetris.recentPlacementTimeMs
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
