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
| Signal references | Role references to externally defined typed signal handles this decision may use for learning, validation, guardrails, objectives, and online inference. | `boardPressureSignal`, `earlyLossRateSignal` |
| Intent | Typed objective: natural-language product direction or metric-driven optimization over declared signals. | natural-language: challenging but playable; metric-objective: minimize early loss |
| Inference | Runtime inference target, app-emitted metric inputs, and fallback order. | target `session`, inputs `boardPressure`, fallback `cohort -> global` |
| Output contract | Result type, bounds, allowed values, step, default. | number, `200..1500`, step `50`, default `800` |
| Safety/policy/guardrails | Hard constraints, operating limits, and requested approval mode. Deployment/environment policy decides whether requested automatic approval is allowed. | gradual, cooldown, min evidence quality, max model uncertainty, requested automatic approval |
| Runtime context schema | Request-time facts the application must or may provide, including fields that identify target levels or metadata. | `sessionId`, `cohort`, `deviceType` |

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
// gameplaySignalHandles.ts
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
  key: "tetris.earlyLossRate24h",
  type: "number",
  from: sessionEndedEvent,
  aggregation: "rate(endReason == 'early_loss')",
  window: "24h"
});

export const hardDropRateSignal = flaggo.metric.derived({
  key: "tetris.hardDropRate24h",
  type: "number",
  from: piecePlacedEvent,
  aggregation: "rate(hardDrop == true)",
  window: "24h"
});
```

Emission imports and uses the typed handle. It should not redefine schemas inside every `emit(...)` call:

```ts
boardPressureSignal.emit(boardPressure);

piecePlacedEvent.emit({
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});
```

Any signal schema or semantic change requires a new key, such as `tetris.boardPressurePercent`. A signal key is immutable; schema digests can detect conflicting duplicate definitions under the same key, but a changed digest must not silently mutate that key's meaning. There is no separate public signal name or revision.

A derived signal's aggregation and window are part of that immutable meaning. For example, `tetris.earlyLossRate24h` always means the declared 24-hour rate. Changing its aggregation or window requires a new key such as `tetris.earlyLossRate7d`; an evidence view must not override the fixed window of a derived signal. Raw events and app-emitted metrics may still be viewed over decision-specific windows.

## SDK-facing shape

The simple code-first UX should hide most contract machinery:

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
  requestedApproval: "automatic",
  context: {
    sessionId: flaggo.target.session(sessionId),
    userId: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort),
    deviceType: device.type
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
await flaggo.exposures.confirm(dropIntervalDecision.decisionId);
```

The code-first object is partitioned by tooling into a versioned decision definition and a runtime request. Emission is global to the application, but association is decision-specific: `signals.evidence`, `intent`, bound `inference.inputs`, and guardrail references declare which signal handles this decision may use. `boardPressureSignal.input(boardPressure)` contributes the signal identity to the extracted definition and the current value to the runtime request. Typed context wrappers such as `flaggo.target.session(sessionId)` similarly contribute target schema plus the current target ID. Plain values remain runtime metadata. Runtime values are excluded from definition digests and revisions. The `flaggo.tune.number(...)` surface returns a number decision receipt: application code applies `.value`, while `.decisionId` supports exposure confirmation.

The code-first `policy` shorthand is normalized to canonical `InlinePolicy` constraints before hashing. For example, `maxDelta: 50` becomes `{ kind: "max-delta", value: 50 }`, and `cooldown: "20s"` becomes `{ kind: "cooldown", seconds: 20 }`. The explicit form may provide `PolicyReference | InlinePolicy` directly; equivalent shorthand and canonical policies produce the same definition digest.

Only app-emitted primitive metric handles may appear in `inference.inputs`. Events and service-derived metrics may contribute to evidence; numeric derived metrics may also serve as objectives, but neither events nor derived metrics can be supplied as online request values. SDK typing enforces this for code-first authoring, while extraction, registry validation, and the Decision API enforce it at trust boundaries.

Metric objectives are narrower than general signal roles: objective signals must be numeric metrics. They may be app-emitted or derived, but events and boolean/string metrics are invalid because `minimize`, `maximize`, and numeric `target` require a numeric domain. SDKs expose a branded numeric metric identity; the registry and Decision API resolve the key and validate its registered declaration.

Objective direction is a discriminated contract. `direction: "target"` requires a finite numeric `target`; `minimize` and `maximize` forbid `target`. SDK typing catches this during authoring, and canonical, registry, and Decision API validation enforce it for language-neutral clients.

Policy is required in both combined and explicit definitions. Code-first `PolicyAuthoring` normalizes to `InlinePolicy`; the explicit form must supply `PolicyReference | InlinePolicy`. No implicit environment/default policy is inserted when policy is omitted.

Code-first extraction is fail-closed. Static semantics must use the SDK's extractable literal subset; spreads, conditional definition fields, computed keys, dynamic signal arrays, helper-returned fragments, and post-construction mutation are invalid for MVP extraction. Runtime expressions are permitted only where the extractor can separate them from static semantics, such as signal/target bindings or statically typed context values. Unsupported syntax fails build/CI instead of producing a runtime-dependent definition.

Tooling extracts and hashes each call site's static descriptor once. Repeated runtime calls rebuild only bound values and attach the cached identity. Identical canonical definitions for the same decision key within one build are deduplicated; different canonical digests for the same key are a `contract-conflict` build error. Combined and explicit authoring forms use the same [canonical normalization and digest rules](design/shared-contracts/README.md#canonical-definition-normalization-and-digest).

Automatic approval requires executable objectives. If a definition requests `requestedApproval: "automatic"`, the definition should use `intent.type: "metric-objective"` and typed policy constraints; natural-language-only intent should require human approval or policy-default handling.

Signal handles are the single declaration surface for facts Flaggo may understand:

| Concept | Source | Used by | Example |
| --- | --- | --- | --- |
| Declared event | Domain event emitted through a typed signal handle. | Async learning, evidence views, validation, audit. | `piecePlacedEvent` |
| App-emitted metric | Application-computed metric with stable semantics. | Async learning and, if selected, online inference. | `boardPressureSignal` |
| Derived signal | Metric declared from other signal handles and an aggregation expression. | Async learning, evidence views, validation, policy. | `earlyLossRateSignal` |
| Inference input | App-emitted metric bound to its current value inside the inference declaration. | Online inference and strategy execution. | `boardPressureSignal.input(boardPressure)` |
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
  primary: { signal: earlyLossRateSignal, direction: "minimize" },
  secondary: [
    { signal: hardDropRateSignal, direction: "target", target: 0.45 },
    { signal: recentPlacementTimeMsSignal, direction: "minimize" }
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
      - tetris.earlyLossRate24h
      - tetris.hardDropRate24h
  intent:
    type: metric-objective
    primary:
      signal: tetris.earlyLossRate24h
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
