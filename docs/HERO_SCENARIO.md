# Hero Scenario: Tetris Drop Speed Decision

## Purpose

This document describes the first design artifact for Flaggo from the user's point of view. It is not an implementation design. It defines what the product should feel like when an application uses AI-native runtime decisioning as a software primitive.

The hero scenario uses a Tetris game because the decision is concrete, observable, and easy to reason about:

> The game should adapt `dropInterval` so each player gets a challenging but playable experience.

## Scenario summary

A Tetris frontend emits gameplay telemetry. Instead of hard-coding one global drop speed or manually tuning a feature flag, the developer declares:

- what runtime value can be decided,
- what telemetry describes success or failure,
- which direction the desired metrics should move,
- what bounds and fallback values keep the experience safe.

At runtime, the game asks Flaggo for the current `dropInterval` decision. Flaggo uses runtime context, recent session telemetry, approved decision strategy, goals, policy constraints, system state, and uncertainty to return a governed value. The game applies the value, emits outcomes, and Flaggo learns from subsequent behavior.

The key product behavior is real-time adaptation, not just choosing a better initial default. Async intelligence can learn and propose a bounded strategy for how drop speed should adapt. The online runtime path can then execute that approved strategy quickly during gameplay.

The scenario uses the decision-factor vocabulary from [Decision Factors for AI-Native Runtime Decisioning](DECISION_FACTORS.md):

| Category | Tetris example |
|---|---|
| Decision surface | `tetris.dropInterval` |
| Runtime context | `userId`, `sessionId`, `currentLevel`, `deviceType` |
| Telemetry evidence | hard-drop rate, placement time, early game-over rate |
| Goals | keep hard-drop rate near target; reduce early losses |
| Policy constraints | min/max value, max delta, cooldown, confidence floor, sample-size minimum |
| System state | current interval, previous decision, cooldown state, operator mode |
| Uncertainty | confidence, sample size, data freshness, conflicting signals |
| Action space | numeric interval from `200ms` to `1500ms` in `50ms` steps; strategy may further narrow range for a segment |
| Fallback contract | use `800ms` when decisioning is unavailable or unsafe |
| Audit/explanation | returned value, reason, evidence snapshot, policy result |

## The user experience we want

### Developer experience

The developer should not have to build an experimentation platform, telemetry pipeline, metrics aggregation layer, policy engine, or decision loop by hand.

The developer should be able to express three application-owned things in code:

1. **Telemetry emission**: the runtime events Flaggo should observe.
2. **Metrics**: the server-side aggregations Flaggo should calculate from those events.
3. **Decision primitive**: the runtime value or behavior that Flaggo may decide.

The important experience is that telemetry is declared and emitted through the Flaggo library. The application records meaningful domain events, and Flaggo handles delivery, aggregation, decision evidence, and audit linkage.

Example intent, not final API:

```ts
const flaggo = createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000,
    includeDecisionContext: true
  }
});
```

The developer defines domain events once:

```ts
const hardDropPressed = flaggo.events.define("hard_drop_pressed", {
  properties: {
    userId: "string",
    sessionId: "string",
    pieceType: "string",
    dropInterval: "number",
    level: "number"
  }
});

const piecePlaced = flaggo.events.define("piece_placed", {
  properties: {
    userId: "string",
    sessionId: "string",
    placementTimeMs: "number",
    placementMethod: "string",
    dropInterval: "number"
  }
});

const gameEnded = flaggo.events.define("game_ended", {
  properties: {
    userId: "string",
    sessionId: "string",
    endReason: "string",
    durationSeconds: "number",
    dropInterval: "number"
  }
});
```

When the player presses hard drop, the game emits the event through the library. The developer does not hand-roll a telemetry pipeline:

```ts
function onHardDrop(piece: Tetromino) {
  gameEngine.hardDrop();

  hardDropPressed.emit({
    userId,
    sessionId,
    pieceType: piece.type,
    dropInterval: gameEngine.config.dropInterval,
    level: gameEngine.level
  });
}
```

The developer then defines metrics as server-side aggregations over emitted events:

```ts
const gameSignals = flaggo.metrics.define({
  hardDropRate: {
    numerator: hardDropPressed.count(),
    denominator: piecePlaced.count(),
    window: "5m",
    direction: "target",
    target: 0.45
  },
  placementTimeMs: {
    value: piecePlaced.property("placementTimeMs").average(),
    window: "5m",
    direction: "minimize"
  },
  earlyGameOverRate: {
    numerator: gameEnded.where({ endReason: "early_loss" }).count(),
    denominator: gameEnded.count(),
    window: "5m",
    direction: "minimize"
  }
});
```

Flaggo receives the events, aggregates the metrics, and uses those metrics as decision evidence. The application does not need to compute `hardDropRate` locally or coordinate a separate analytics job before asking for a decision.

Finally, the developer declares the decision primitive that uses those metrics:

```ts
const dropInterval = flaggo.decision.number("tetris.dropInterval", {
  actionSpace: {
    default: 800,
    min: 200,
    max: 1500,
    step: 50
  },
  goals: [
    gameSignals.hardDropRate.near(0.45),
    gameSignals.placementTimeMs.minimize(),
    gameSignals.earlyGameOverRate.minimize()
  ],
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minConfidence: 0.7
  },
  onlineStrategy: {
    mode: "approved-strategy",
    liveInputs: [
      "currentLevel",
      "boardPressure",
      "recentPlacementTimeMs",
      "recoveryFailures"
    ]
  },
  fallback: {
    value: 800,
    strategy: "use-default"
  }
});
```

System state is intentionally not declared by the application. Flaggo owns state such as the current active value, previous decision, cooldown status, operator mode, and rollback state.

At runtime, application code should feel simple:

```ts
const interval = await dropInterval.decide({
  runtimeContext: {
    userId,
    sessionId,
    currentLevel,
    deviceType,
    boardPressure,
    recentPlacementTimeMs,
    recoveryFailures
  }
});

gameEngine.updateConfig({ dropInterval: interval.value });
```

The important design principle is that the decision is declared directly and explicitly. The application does not hide adaptive behavior behind scattered `if/else` branches. It names the decision surface, runtime context shape, telemetry evidence, goals, action space, policy constraints, and fallback contract.

In the adaptive version of the scenario, the developer still asks for one value:

```ts
gameEngine.updateConfig({ dropInterval: interval.value });
```

But Flaggo may produce that value by executing an approved strategy:

```text
current value = 800ms
board pressure = high
recent placement time = slow
recovery failures = 2
approved strategy = slow down by one step when pressure is high and recovery is poor

returned value = 850ms
```

This keeps the game code simple while allowing runtime behavior to adapt to the current session.

### Operator and product experience

The operator should be able to inspect and govern the decision without reading application code.

For `tetris.dropInterval`, the operator should see:

- the decision surface: `tetris.dropInterval`,
- the declared goal: keep gameplay challenging but playable,
- the action space: `200ms` to `1500ms` in `50ms` steps,
- the fallback contract: `800ms`,
- the active policy constraints: confidence floor, sample-size minimum, max change per decision, cooldown, and guardrail limits,
- the current system state: active value, previous value, cooldown state, and operator mode,
- the uncertainty state: confidence, evidence freshness, sample size, and conflicting signals,
- recent decisions and explanations,
- whether the decision is observing, suggesting, or applying changes,
- the active decision strategy, if one is approved,
- the latest strategy proposal and why it was accepted, limited, or rejected,
- controls to pause, resume, override, or roll back.

The operator experience matters because Flaggo is not just a metric optimizer. It is a governed runtime decision layer. Human intent must remain visible in goals, boundaries, and operating mode.

### End-user experience

The player should not experience random or chaotic changes. The game should feel like it is adapting thoughtfully:

- if the game is too slow, pieces may fall faster over time,
- if the game is too punishing, pieces may fall slower,
- if evidence is weak or contradictory, the game should remain stable,
- if policies block adaptation, the player should receive the safe fallback behavior.

For example:

```text
New session starts:
  return 800ms

Player is near the top of the board and placing pieces slowly:
  return 850ms or 900ms within max-delta and cooldown limits

Player stabilizes after recovery:
  return 800ms or 750ms as pressure decreases

Player is skilled and consistently stable:
  return 700ms if the active strategy allows speed-up
```

The end user does not need to know Flaggo exists, but they should benefit from behavior that is more contextual than static configuration.

## How the scenario covers the manifesto principles

### Decisions are explicit rather than hidden in scattered code paths

The adaptive behavior is represented by a named decision: `tetris.dropInterval`.

The developer declares the decision as a first-class primitive rather than manually spreading logic through the game loop:

```text
tetris.dropInterval = governed runtime decision
```

This makes the decision discoverable, testable, observable, and governable. A developer, operator, or auditor can ask: what runtime decisions exist in this application?

### Runtime context and telemetry evidence can influence behavior

The decision uses two kinds of information:

- **Runtime context**: user, session, level, device type, segment, environment.
- **Telemetry evidence**: hard-drop rate, placement time, game duration, early losses, configuration changes.

The decision is not based on a static flag alone. It can account for how this user or segment is actually experiencing the game.

### Policies and constraints are first-class

The decision is never just "whatever the model thinks is best." It is bounded by explicit constraints:

- action-space bounds: minimum and maximum drop interval,
- action-space granularity: step size,
- maximum delta per decision,
- cooldown between changes,
- minimum evidence requirements,
- confidence threshold,
- guardrail metrics,
- operator-controlled mode.

Policy is not an afterthought. It is part of the decision contract.

### Uncertainty is acknowledged instead of ignored

Flaggo should not pretend every recommendation is equally reliable.

For each decision, the system should expose:

- confidence,
- evidence volume,
- evidence freshness,
- competing interpretations,
- whether the decision was applied, suggested, or blocked,
- why fallback was used.

In the Tetris example, high hard-drop rate may mean the game is too slow, but it may also mean an expert player is intentionally playing fast. Flaggo must represent this uncertainty instead of converting weak evidence into automatic action.

### Decisions are explainable and auditable

Every returned value should be explainable after the fact:

```text
Decision: tetris.dropInterval
Scope: user:123
Previous value: 800
Returned value: 700
Reason: hard-drop rate remained above target with sufficient recent evidence
Confidence: 0.72
Policy result: approved
Fallback used: no
```

Auditability means the team can reconstruct what happened, why it happened, which policy allowed it, and what evidence supported it.

### Human intent remains encoded in goals and boundaries

The developer and operator do not ask Flaggo to "make the game better" in an open-ended way.

They encode intent:

- keep hard-drop rate near a target,
- reduce early losses,
- preserve playable bounds,
- avoid frequent changes,
- prefer stability when evidence is weak.

AI-native decisioning should amplify human intent, not replace it.

### Fallback behavior exists when confidence, evidence, or safety is insufficient

The game must always have a safe behavior even when Flaggo cannot decide.

Fallback should be used when:

- the service is unavailable,
- telemetry is missing,
- sample size is too small,
- confidence is too low,
- policy blocks the proposal,
- the requested runtime context is invalid,
- the decision is paused by an operator.

For this scenario, fallback is simple:

```text
tetris.dropInterval -> 800ms
```

Fallback is part of the primitive, not an exception path left to each developer to rediscover.

## Desired runtime loop

```text
Developer declares events, metrics, decision surface, action space, goals, policy, and fallback
        ↓
Application emits telemetry
        ↓
Application asks Flaggo for runtime decision with runtime context
        ↓
Flaggo evaluates runtime context, telemetry evidence, goals, policy constraints, system state, and uncertainty
        ↓
Flaggo returns value + explanation + audit record
        ↓
Application applies value or fallback
        ↓
Telemetry records outcome
        ↓
Future decisions improve or remain stable
```

## What this artifact intentionally does not define

This document does not define:

- final SDK syntax,
- service APIs,
- storage schema,
- model architecture,
- policy implementation,
- frontend screen layout,
- rollout algorithm.

Those should come later. The purpose here is to anchor the design around the desired experience and the product primitive.

## Design implication

The first Flaggo design should be organized around this question:

> What is the smallest complete system that lets a developer declare a governed runtime decision, lets an application ask for that decision, and lets an operator understand why the decision happened?

For the Tetris scenario, that means the first product slice should make `tetris.dropInterval` explicit as a decision surface, driven by runtime context and telemetry evidence, bounded by action space and policy constraints, aware of system state and uncertainty, explainable through audit context, and safe by fallback contract.
