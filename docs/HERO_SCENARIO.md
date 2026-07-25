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

The developer should not have to build an experimentation platform, telemetry pipeline, metrics aggregation layer, policy engine, contract registry workflow, or decision loop by hand before seeing value.

The primary developer loop should stay small:

```text
declare -> decide -> observe
```

The TypeScript hero path should ask the developer to express only three things:

1. **Declare** the bounded runtime value and its safety envelope.
2. **Decide** by asking for a value with live gameplay context.
3. **Observe** meaningful outcomes after the value is used.

This code-first path is an ergonomic authoring mode, not the only control-plane model. The same decision contract should also be expressible through a language-neutral `flaggo.contract-bundle.json` for bundle-first, registry-first, GitOps, or direct REST-client workflows.

The important experience is that the adaptive value is easy to declare and use in application code, while contract synchronization, registry revisions, runtime compatibility checks, strategy activation, evidence correlation, policy expansion, and audit linkage remain control-plane concerns.

Example setup intent, not final API:

```ts
const flaggo = createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  contract: {
    expectedDigest: process.env.FLAGGO_CONTRACT_DIGEST,
    expectedRevision: process.env.FLAGGO_CONTRACT_REVISION
  },
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000,
    includeDecisionContext: true
  }
});
```

The runtime SDK carries only compact contract identity, such as expected digest or revision. It does not send the full contract bundle on every decision request.

The developer declares one adaptive value:

```ts
const dropInterval = flaggo.tune.number("tetris.dropInterval", {
  default: 800,
  range: [200, 1500],
  step: 50,
  optimize: "challenging-but-playable",
  safety: "gradual"
});
```

`default` is the safe value returned when Flaggo cannot provide an approved value. The developer should not need to declare a second fallback value in the basic path.

At runtime, application code should feel simple:

```ts
const sessionDropInterval = dropInterval.forSession(sessionId);

const interval = await sessionDropInterval.get({
  level: game.level,
  boardPressure,
  recentPlacementTimeMs,
  recoveryFailures
});

gameEngine.updateConfig({ dropInterval: interval });
```

Then the game reports what happened:

```ts
sessionDropInterval.observe("piece_placed", {
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});

sessionDropInterval.observe("session_ended", {
  reason: endReason,
  durationSeconds
});
```

The SDK automatically associates observations with the decision surface, returned value, session scope, decision correlation ID, timestamp, and contract revision. The developer does not manually include `dropInterval`, `userId`, or `sessionId` on every observation unless they need to override inferred values.

The important design principle is that the decision is declared directly and explicitly. The application does not hide adaptive behavior behind scattered `if/else` branches. It names or references the decision surface, action space, safety preset, and optimization intent. The control plane expands those into contracts, policy, evidence requirements, approved strategies, and audit records.

In the adaptive version of the scenario, the developer still applies one value:

```ts
gameEngine.updateConfig({ dropInterval: interval });
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

### Advanced evidence and governance mode

Once a team needs exact control, it can graduate to explicit evidence and governance configuration:

```ts
const dropInterval = flaggo.tune.number("tetris.dropInterval", {
  default: 800,
  range: [200, 1500],
  step: 50,
  optimize: {
    primary: signals.earlyLossRate.minimize(),
    secondary: [
      signals.hardDropRate.near(0.45),
      signals.placementTimeMs.minimize()
    ]
  },
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minConfidence: 0.7
  }
});
```

This retains the system's depth without charging every user the full conceptual cost on day one. Named domain events, reusable metric definitions, OpenTelemetry bindings, and warehouse-backed evidence should be advanced evidence modes, not prerequisites for the first successful adaptive value.

System state and strategy are intentionally not declared by the application in the basic path. Flaggo owns state such as the current active value, previous decision, cooldown status, operator mode, rollback state, and active strategy. The developer says what should be optimized and what is safe; Flaggo and operators decide whether that is currently served by a fixed value, numeric rule, experiment, learned strategy, or fallback-only mode.

The declaration can produce or contribute to a canonical contract bundle during build or release:

```text
TypeScript declarations, hand-authored YAML/JSON, or registry export
  -> flaggo.contract-bundle.json
  -> flaggo contracts validate
  -> flaggo contracts apply
  -> registration receipt
  -> deployed workload carries expected digest/revision
```

### Software lifecycle experience

Flaggo should support different owners and systems across the software lifecycle. The SDK is important in development and runtime, but it should not be the only way to synchronize contracts.

| Stage | Developer or platform action | Flaggo artifact | SDK/runtime role |
| --- | --- | --- | --- |
| Development | Author decision declaration in TypeScript, JSON/YAML, or registry UI. | Local declaration or draft contract bundle. | SDK provides ergonomic code-first declarations and local fallback typing. |
| Build | Extract or assemble canonical contract bundle. | `flaggo.contract-bundle.json` plus stable digest. | SDK extractor may generate the bundle, but manifest-first and registry-first workflows can produce the same artifact without SDK execution. |
| CI/release | Validate and apply/promote bundle. | Registration receipt with revision and digest. | Standalone CLI or automation talks to registry; application runtime SDK is not required. |
| Deployment | Attach compact contract identity to workload. | Expected digest, expected revision, build/deployment ID. | Identity can be injected via environment variables, generated constants, container labels, annotations, or direct REST headers. |
| Runtime | Ask for decisions and emit telemetry. | `DecideRequest` with expected contract identity; `DecideResponse` with contract integrity status. | SDK sends compact identity, runtime context, and telemetry; Decision API verifies integrity before approval. |
| Observe/operate | Inspect drift, audit, fallback, and strategy behavior. | Audit records, diagnostics, integrity metrics, operator warnings. | SDK exposes response fields; control plane owns audit, strategy, policy, and operator actions. |

This lifecycle supports TypeScript-first development without making TypeScript SDK extraction a global architectural requirement.

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
