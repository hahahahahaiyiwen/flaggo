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

At runtime, the game asks Flaggo for the current `dropInterval` decision for a runtime target such as the current session. Flaggo uses the versioned decision definition, runtime context, evidence views, approved governed state, goals, policy constraints, and uncertainty to return a governed value. The game applies the value, emits outcomes, and Flaggo learns from subsequent behavior.

The key product behavior is real-time adaptation, not just choosing a better initial default. Async intelligence can learn and propose a bounded strategy for how drop speed should adapt. The online runtime path can then execute that approved strategy quickly during gameplay.

The scenario uses the three-part mental model from [Mental Model](MENTAL_MODEL.md), [Decision Definition](DECISION_DEFINITION.md), [Decision Evidence](DECISION_EVIDENCE.md), and [Decision Intelligence](DECISION_INTELLIGENCE.md):

| Category | Tetris example |
|---|---|
| Decision surface/key | `tetris.dropInterval` |
| Decision definition | SDK/registry-managed revision such as `tetris.dropInterval@2` |
| Runtime target | `session:game-456` |
| Control target | `cohort:new_players` or `global` |
| Runtime context | `userId`, `sessionId`, `cohort`, `currentLevel`, `deviceType`, `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures` |
| Evidence views | hard-drop rate, placement time, early game-over rate by session/cohort/global windows |
| Goals | keep hard-drop rate near target; reduce early losses |
| Policy constraints | min/max value, max delta, cooldown, minimum evidence quality, maximum model uncertainty, sample-size minimum |
| Governed state | active strategy, current interval, previous decision, cooldown state, rollout, operator mode |
| Uncertainty | evidence quality, model uncertainty, expected outcome, sample size, data freshness, conflicting signals |
| Action space | numeric interval from `200ms` to `1500ms` in `50ms` steps; strategy may further narrow range for a segment |
| Fallback contract | use `800ms` when decisioning is unavailable or unsafe |
| Audit/explanation | returned value, reason, evidence snapshot, policy result |

## The user experience we want

### Developer experience

The developer should not have to build an experimentation platform, telemetry pipeline, metrics aggregation layer, policy engine, contract registry workflow, or decision loop by hand before seeing value.

The primary developer loop should stay small:

```text
declare -> decide by observing
```

The TypeScript hero path should ask the developer to express two things:

1. **Declare** the bounded decision definition, including target hierarchy, context, evidence, safety, and fallback.
2. **Decide** by asking for a concrete value with live gameplay context. Flaggo links the decision to evidence and outcomes through instrumentation.

This code-first path is an ergonomic authoring mode, not the only control-plane model. The same decision contract should also be expressible through a language-neutral `flaggo.contract-bundle.json` for bundle-first, registry-first, GitOps, or direct REST-client workflows.

The important experience is that the adaptive value is easy to declare and use in application code, while contract synchronization, definition revisions, runtime target resolution, control targets, strategy activation, evidence correlation, policy expansion, and audit linkage remain control-plane concerns.

Example setup intent, not final API:

```ts
const flaggo = createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  contract: {
    expectedContractDigest: process.env.FLAGGO_CONTRACT_DIGEST,
    expectedRevision: process.env.FLAGGO_CONTRACT_REVISION,
    buildId: process.env.BUILD_ID,
    deploymentId: process.env.DEPLOYMENT_ID
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

The runtime SDK carries only compact contract/build identity, such as expected contract digest, revision, build ID, or deployment ID. It does not send the full contract bundle on every decision request.

The developer defines gameplay signals once near their producer, then asks for one adaptive value by referencing those typed handles.

```ts
export const boardPressureSignal = flaggo.metric.number({
  key: "tetris.boardPressure",
  range: [0, 1]
});

export const recentPlacementTimeMsSignal = flaggo.metric.number({
  key: "tetris.recentPlacementTimeMs",
  unit: "ms"
});

export const recoveryFailuresSignal = flaggo.metric.number({
  key: "tetris.recoveryFailures"
});

export const currentLevelSignal = flaggo.metric.number({
  key: "tetris.currentLevel"
});

export const piecePlacedEvent = flaggo.event({
  key: "tetris.piecePlaced",
  fields: {
    placementTimeMs: flaggo.number({ unit: "ms" }),
    hardDrop: flaggo.boolean()
  }
});

export const sessionEndedEvent = flaggo.event({
  key: "tetris.sessionEnded",
  fields: {
    endReason: flaggo.string(),
    durationSeconds: flaggo.number({ unit: "s" })
  }
});

export const earlyLossRateSignal = flaggo.metric.derived({
  key: "tetris.earlyLossRate",
  type: "number",
  from: sessionEndedEvent,
  aggregation: "rate(endReason == 'early_loss')",
  window: "24h"
});

export const hardDropRateSignal = flaggo.metric.derived({
  key: "tetris.hardDropRate",
  type: "number",
  from: piecePlacedEvent,
  aggregation: "rate(hardDrop == true)",
  window: "24h"
});
```

The decision definition does not own those schemas. It directly references individual typed signal handles by role; tooling can derive an explicit associated-signal set in the extracted contract. Emitting a signal does not associate it with every decision in the program: each decision opts into the signals it may use for evidence, objectives, inference, and guardrails.

```ts
const dropIntervalDecision = await flaggo.tune.number("tetris.dropInterval", {
  targetHierarchy: ["session", "user", "cohort", "global"],
  signals: {
    evidence: [
      piecePlacedEvent,
      sessionEndedEvent,
      earlyLossRateSignal,
      hardDropRateSignal
    ]
  },
  intent: {
    type: "metric-objective",
    primary: { signal: earlyLossRateSignal, direction: "minimize" },
    secondary: [
      { signal: hardDropRateSignal, direction: "target", target: 0.45 },
      { signal: recentPlacementTimeMsSignal, direction: "minimize" }
    ],
    rationale: "Keep gameplay challenging but playable while reducing early frustration."
  },
  inference: {
    target: "session",
    inputs: [
      boardPressureSignal.input(boardPressure),
      recentPlacementTimeMsSignal.input(recentPlacementTimeMs),
      recoveryFailuresSignal.input(recoveryFailures),
      currentLevelSignal.input(game.level)
    ],
    fallbackOrder: ["cohort", "global"]
  },
  output: {
    default: 800,
    range: [200, 1500],
    step: 50
  },
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minEvidenceQuality: 0.7,
    maxModelUncertainty: 0.35
  },
  context: {
    session: flaggo.target.session(sessionId),
    user: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort),
    deviceType: device.type
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
await flaggo.exposures.confirm(dropIntervalDecision.decisionId);
```

In this shape, `flaggo.tune.number(...)` keeps the original SDK surface but returns a number decision object. The application still applies a plain numeric value through `dropIntervalDecision.value`, while the SDK exposes the decision receipt needed for attribution. If a value-only convenience is needed later, it should be a separate helper or projection that intentionally opts out of closed-loop exposure attribution.

The code-first object combines authoring and invocation without conflating their persisted forms. A bound input such as `boardPressureSignal.input(boardPressure)` contributes the immutable signal reference to the extracted definition and the current value to the runtime request. A typed target such as `flaggo.target.session(sessionId)` contributes the target kind to the extracted context schema and the current ID to the runtime request. Plain context values such as `deviceType` remain runtime metadata. Flaggo excludes bound runtime values from definition digests and revisions.

`signals.evidence` declares emitted or derived signals that this decision may use for evidence and learning; emitting a signal does not associate it with every decision. `inference.target` declares the desired target kind, and `inference.fallbackOrder` keeps resolution explicit. Derived signals such as `earlyLossRateSignal` declare their typed source and aggregation separately. The registry can still govern behavior at a broader control target such as `cohort:new_players`. `output.default` is the safe value returned when Flaggo cannot provide an approved value.

This keeps the online path simple: the application sends pre-aggregated metric values, and the service does not aggregate them on the hot path. The same metric values can be emitted over time for async learning, captured in the decision record when a value is returned, and captured in an exposure record only after the client confirms the value was applied or rendered.

The application can continue emitting normal domain events or OpenTelemetry signals:

```ts
boardPressureSignal.emit(boardPressure);
recentPlacementTimeMsSignal.emit(recentPlacementTimeMs);
recoveryFailuresSignal.emit(recoveryFailures);

piecePlacedEvent.emit({
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});

sessionEndedEvent.emit({
  endReason,
  durationSeconds
});
```

The SDK and telemetry pipeline automatically associate matching observations with the decision key, definition revision, returned value, runtime target, decision correlation ID, timestamp, and application/build provenance. The developer does not need a separate decision-scoped observe step in the hero path unless they want an explicit shorthand.

The important design principle is that the decision is declared directly and explicitly. The application does not hide adaptive behavior behind scattered `if/else` branches. It names or references the decision key, output contract, target hierarchy, safety preset, and optimization intent. The control plane expands those into versioned definitions, policy, evidence requirements, approved strategies, and audit records.

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
const dropIntervalDecision = await flaggo.tune.number("tetris.dropInterval", {
  definition: {
    targetHierarchy: ["session", "user", "cohort", "global"],
    signals: {
      evidence: [
        piecePlacedEvent,
        sessionEndedEvent,
        earlyLossRateSignal,
        hardDropRateSignal
      ]
    },
    intent: {
      type: "metric-objective",
      primary: { signal: earlyLossRateSignal, direction: "minimize" },
      secondary: [
        { signal: hardDropRateSignal, direction: "target", target: 0.45 },
        { signal: recentPlacementTimeMsSignal, direction: "minimize" }
      ],
      rationale: "Keep the game challenging while reducing early frustration."
    },
    inference: {
      target: "session",
      inputs: [
        boardPressureSignal,
        recentPlacementTimeMsSignal,
        recoveryFailuresSignal,
        currentLevelSignal
      ],
      fallbackOrder: ["cohort", "global"]
    },
    output: {
      default: 800,
      range: [200, 1500],
      step: 50
    },
    policy: {
      maxDelta: 50,
      cooldown: "20s",
      minSampleSize: 30,
      minEvidenceQuality: 0.7,
      maxModelUncertainty: 0.35
    },
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
    deviceType: device.type
  },
  inputs: [
    boardPressureSignal.input(boardPressure),
    recentPlacementTimeMsSignal.input(recentPlacementTimeMs),
    recoveryFailuresSignal.input(recoveryFailures),
    currentLevelSignal.input(game.level)
  ]
});
```

This explicit form remains useful when definitions are generated, reused across call sites, registered outside application execution, or authored independently from runtime values. It maps directly to the separated definition and request contracts. The combined code-first form should remain the default UX.

Governed state and strategy are intentionally not declared by the application in the basic path. Flaggo owns state such as the current active value, previous decision, cooldown status, rollout, operator mode, rollback transition metadata, and active strategy. The developer says what should be optimized and what is safe; Flaggo and operators decide whether that is currently served by a fixed value, numeric rule, experiment, learned strategy, or fallback-only mode.

The declaration can produce or contribute to a canonical contract bundle during build or release:

```text
TypeScript declarations, hand-authored YAML/JSON, or registry export
  -> flaggo.contract-bundle.json
  -> flaggo contracts validate
  -> flaggo contracts apply
  -> registration receipt
  -> each deployed workload carries its own expected contract/build identity
```

### Software lifecycle experience

Flaggo should support different owners and systems across the software lifecycle. The SDK is important in development and runtime, but it should not be the only way to synchronize contracts.

| Stage | Developer or platform action | Flaggo artifact | SDK/runtime role |
| --- | --- | --- | --- |
| Development | Author decision declaration in TypeScript, JSON/YAML, or registry UI. | Local declaration or draft contract bundle. | SDK provides ergonomic code-first declarations and local fallback typing. |
| Build | Extract or assemble canonical contract bundle for that build. | `flaggo.contract-bundle.json`, `contractDigest`, optional `buildId` and `artifactDigest`. | SDK extractor may generate the bundle, but manifest-first and registry-first workflows can produce the same artifact without SDK execution. |
| CI/release | Validate and apply/promote bundle. | Registration receipt with bundle digest, contract digest, and decision definition revisions. | Standalone CLI or automation talks to registry; application runtime SDK is not required. |
| Deployment | Attach compact contract/build identity to each workload version. | Expected contract digest, bundle digest, revision, build ID, deployment ID. | Identity can be injected via environment variables, generated constants, container labels, annotations, or direct REST headers. |
| Runtime | Ask for decisions and emit telemetry. | `DecideRequest` with that workload's expected contract identity; `DecideResponse` with contract integrity status. | SDK sends compact identity, runtime context, and telemetry; Decision API verifies the calling build's known contract before approval. |
| Observe/operate | Inspect drift, audit, fallback, and strategy behavior. | Audit records, diagnostics, integrity metrics, operator warnings. | SDK exposes response fields; control plane owns audit, strategy, policy, and operator actions. |

This lifecycle supports TypeScript-first development without making TypeScript SDK extraction a global architectural requirement. It also supports rolling deployments where two builds of the same service are live at the same time: each build carries its own expected contract identity, and Flaggo recognizes known immutable contract IDs/revisions instead of forcing every build onto one current contract.

When a definition changes semantically, Flaggo should not automatically share active decision state with the new definition. It may still reuse telemetry facts and matching immutable signal keys so the new definition does not start completely cold:

```text
raw observations: reusable when immutable signal keys match
evidence views: reusable when signal key, target, window, and filters match
governed state: isolated by decision definition and control target
runtime target state: isolated by decision definition and runtime target
```

This lets a new `dropInterval` definition add a signal such as `recoveryFailures` while reusing historical `boardPressure` and `placementTimeMs` evidence. The new signal warms up independently, and active strategies remain isolated until an explicit migration is approved.

### Operator and product experience

The operator should be able to inspect and govern the decision without reading application code.

For `tetris.dropInterval`, the operator should see:

- the decision key: `tetris.dropInterval`,
- the active definition revision, such as `tetris.dropInterval@2`,
- the runtime targets receiving decisions, such as sessions,
- the control targets where behavior is governed, such as `cohort:new_players`,
- the declared goal: keep gameplay challenging but playable,
- the action space: `200ms` to `1500ms` in `50ms` steps,
- the fallback contract: `800ms`,
- the active policy constraints: minimum evidence quality, maximum model uncertainty, sample-size minimum, max change per decision, cooldown, and guardrail limits,
- the current governed state: active value, previous value, cooldown state, rollout, and operator mode,
- the uncertainty state: evidence quality, model uncertainty, expected outcome, evidence freshness, sample size, and conflicting signals,
- recent decisions and explanations,
- whether the decision is observing, suggesting, or applying changes,
- the active decision strategy, if one is approved,
- the latest strategy proposal and why it was accepted, limited, or rejected,
- controls to pause, resume, override, or roll back.

The operator experience matters because Flaggo is not just a metric optimizer. It is a policy-controlled runtime decisioning layer. Human intent must remain visible in goals, boundaries, and operating mode.

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
tetris.dropInterval = named runtime decision family
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
- minimum evidence quality,
- maximum model uncertainty,
- minimum expected outcome when an optimization estimate is used,
- guardrail metrics,
- operator-controlled mode.

Policy is not an afterthought. It is part of the decision contract.

### Uncertainty is acknowledged instead of ignored

Flaggo should not pretend every recommendation is equally reliable.

For each decision, the system should expose:

- evidence quality,
- model uncertainty,
- expected outcome estimate when available,
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
Runtime target: user:123
Previous value: 800
Returned value: 700
Reason: hard-drop rate remained above target with sufficient recent evidence
Evidence quality: 0.82
Model uncertainty: 0.31
Expected outcome: 0.72
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

### Fallback behavior exists when evidence, uncertainty, expected outcome, or safety is insufficient

The game must always have a safe behavior even when Flaggo cannot decide.

Fallback should be used when:

- the service is unavailable,
- telemetry is missing,
- sample size is too small,
- evidence quality is too low,
- model uncertainty is too high,
- expected outcome is below the required threshold,
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
Developer declares a decision key, target, action space, goal, safety preset, and fallback
        ↓
Application emits telemetry
        ↓
Application asks Flaggo for runtime decision with runtime context
        ↓
Flaggo evaluates runtime context, evidence views, governed state, policy constraints, and uncertainty
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

> What is the smallest complete system that lets a developer declare a runtime decision family, lets an application request a RuntimeDecisionResult, and lets an operator understand why the result happened?

For the Tetris scenario, that means the first product slice should make `tetris.dropInterval` explicit as a stable decision key with a versioned definition, driven by runtime context and reusable evidence views, bounded by action space and policy constraints, aware of governed state and uncertainty, explainable through audit context, and safe by fallback contract.
