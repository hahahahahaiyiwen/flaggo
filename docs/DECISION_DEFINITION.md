# Decision Definition

## Purpose

A decision definition is the versioned contract for a Flaggo decision. It
declares what may be decided, which target levels matter, which evidence may be
used, what "better" means, which safety/fallback rules constrain the system,
and how initial or future authority may be supplied.

It participates in Flaggo's top-level mental model:

```text
Decision Definition
Decision Evidence
Decision Intelligence
Decision Lifecycles
Runtime Decision Execution
```

## Definition

A decision definition belongs to a stable decision key.

```text
decision key:
  tetris.dropInterval

definition lineage:
  definitionId: def_01JQ8Y7M6X3K9P2W4R5T6V7N8A

runtime revisions:
  revision: rev_01JQ8Y8A1B2C3D4E5F6G7H8J9K
  revision: rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3
```

The key identifies the developer-facing decision family. `definitionId` is an opaque registry lineage ID. Each approved semantic change creates an opaque runtime `revision` and canonical `contractDigest`; the full tuple is the immutable runtime identity.

## What a decision definition owns

| Part | Meaning | Tetris example |
| --- | --- | --- |
| Decision key | Stable application-facing decision family. | `tetris.dropInterval` |
| Revision | Opaque registry-issued runtime revision; not semantic versioning or a metadata revision. | `rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3` |
| Signal references | Role references to externally defined typed signal handles this decision may use for learning, validation, guardrails, objectives, and declared runtime inputs. | `boardPressureSignal`, `earlyLossRateSignal` |
| Intent | Typed objective: natural-language product direction or metric-driven optimization over declared signals. | natural-language: challenging but playable; metric-objective: minimize early loss |
| Inference | Runtime inference target, app-emitted metric inputs, and fallback order. | target `session`, inputs `boardPressure`, fallback `cohort -> global` |
| Output contract | Result type, bounds, allowed values, step, default. | number, `200..1500`, step `50`, default `800` |
| Safety/policy/guardrails | Hard constraints and operating limits. Evidence-backed constraints apply only when the active authority claims that evidence. | bounds, step, max delta |
| Authority workflow | How approved state is initially supplied and which runtime mechanism it contains. | authenticated bundle approval of a numeric-rule candidate |
| Runtime context schema | Request-time facts the application must or may provide, including fields that identify target levels or metadata. | `sessionId`, `cohort`, `deviceType` |

## What it does not own

A decision definition does not own:

- raw telemetry history,
- signal schemas,
- evidence snapshots,
- app/build provenance,
- approved active strategy or governed state,
- active experiment variants or allocation,
- rollout state,
- concrete runtime decision results,
- audit records.

Those belong to [Decision Evidence](DECISION_EVIDENCE.md), [Decision Intelligence](DECISION_INTELLIGENCE.md), [Decision Lifecycles](DECISION_LIFECYCLES.md), [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md), governed state, and audit/explanation components.

A bundle-approved definition may contain an initial authority candidate. That
candidate participates in semantic identity, but it is not active authority
until an authenticated control-plane approval and state activation succeed.

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
  lifecycle: {
    authorityMode: "bundle-approved",
    initialAuthority: {
      controlTarget: { type: "cohort", id: "new_players" },
      kind: "numeric-rule",
      rule: {
        threshold: 0.55,
        valueAtOrAbove: 850,
        valueBelow: 750,
        weightedInputs: [
          {
            signal: boardPressureSignal,
            minimum: 0,
            maximum: 1,
            weight: 0.45
          },
          {
            signal: recentPlacementTimeMsSignal,
            minimum: 0,
            maximum: 2000,
            weight: 0.25
          },
          {
            signal: recoveryFailuresSignal,
            minimum: 0,
            maximum: 5,
            weight: 0.20
          },
          {
            signal: currentLevelSignal,
            minimum: 0,
            maximum: 20,
            weight: 0.10
          }
        ]
      },
      rationale: "Initial deterministic Tetris behavior."
    }
  },
  policy: {
    maxDelta: 50
  },
  context: {
    sessionId: flaggo.target.session(sessionId),
    userId: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort)
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
if (
  dropIntervalDecision.source === "server" &&
  dropIntervalDecision.exposure.confirmationRequired
) {
  await flaggo.exposures.confirm(
    dropIntervalDecision.decisionId,
    dropIntervalDecision.exposure.confirmToken
  );
}
```

The code-first object is partitioned by tooling into a versioned decision definition and a runtime request. Emission is global to the application, but association is decision-specific: `signals.evidence`, `intent`, bound `inference.inputs`, and guardrail references declare which signal handles this decision may use. `boardPressureSignal.input(boardPressure)` contributes the signal identity to the extracted definition and the current value to the runtime request. Typed context wrappers such as `flaggo.target.session(sessionId)` similarly contribute target schema plus the current target ID. Runtime values are excluded from definition digests and revisions. The `flaggo.tune.number(...)` surface returns a number decision receipt: application code applies `.value`, while its server exposure directive authorizes confirmation.

The code-first `policy` shorthand is normalized to canonical `InlinePolicy`
constraints before hashing. For example, `maxDelta: 50` becomes
`{ kind: "max-delta", value: 50 }`. The explicit form may provide
`PolicyReference | InlinePolicy` directly; equivalent shorthand and canonical
policies produce the same definition digest. Cooldown authoring remains
deferred to #33.

Only app-emitted primitive metric handles may appear in `inference.inputs`. Events and service-derived metrics may contribute to evidence; numeric derived metrics may also serve as objectives, but neither events nor derived metrics can be supplied as online request values. SDK typing enforces this for code-first authoring, while extraction, registry validation, and the Decision API enforce it at trust boundaries.

Metric objectives are narrower than general signal roles: objective signals must be numeric metrics. They may be app-emitted or derived, but events and boolean/string metrics are invalid because `minimize`, `maximize`, and numeric `target` require a numeric domain. SDKs expose a branded numeric metric identity; the registry and Decision API resolve the key and validate its registered declaration.

Objective direction is a discriminated contract. `direction: "target"` requires a finite numeric `target`; `minimize` and `maximize` forbid `target`. SDK typing catches this during authoring, and canonical, registry, and Decision API validation enforce it for language-neutral clients.

Policy is required in both combined and explicit definitions. Code-first `PolicyAuthoring` normalizes to `InlinePolicy`; the explicit form must supply `PolicyReference | InlinePolicy`. No implicit environment/default policy is inserted when policy is omitted.

Code-first extraction is fail-closed. Static semantics must use the SDK's extractable literal subset; spreads, conditional definition fields, computed keys, dynamic signal arrays, helper-returned fragments, and post-construction mutation are invalid for MVP extraction. Runtime expressions are permitted only where the extractor can separate them from static semantics, such as signal/target bindings or statically typed context values. Unsupported syntax fails build/CI instead of producing a runtime-dependent definition.

Tooling extracts and hashes each call site's static descriptor once. Repeated runtime calls rebuild only bound values and attach the cached identity. Identical canonical definitions for the same decision key within one build are deduplicated; different canonical digests for the same key are a `contract-conflict` build error. Combined and explicit authoring forms use the same [canonical normalization and digest rules](design/shared-contracts/README.md#canonical-definition-normalization-and-digest).

The bundle cannot request trusted authority for itself. A control-plane actor
approves the exact semantic snapshot. Proposal-managed automatic approval, when
introduced later, still requires executable objectives, typed policy
constraints, and environment authority.

Signal handles are the single declaration surface for facts Flaggo may understand:

| Concept | Source | Used by | Example |
| --- | --- | --- | --- |
| Declared event | Domain event emitted through a typed signal handle. | Async learning, evidence views, validation, audit. | `piecePlacedEvent` |
| App-emitted metric | Application-computed metric with stable semantics. | Async learning and, if selected, runtime strategy evaluation. | `boardPressureSignal` |
| Derived signal | Metric declared from other signal handles and an aggregation expression. | Async learning, evidence views, validation, policy. | `earlyLossRateSignal` |
| Inference input | App-emitted metric bound to its current value inside the inference declaration. | Runtime strategy evaluation. | `boardPressureSignal.input(boardPressure)` |
| Decision-record input | Inference input value captured when a value is returned. | Auditing what Flaggo decided for the request. | `decision.boardPressure` when `850ms` was returned |
| Exposure-captured input | Inference input value copied to an exposure only after the client narrows to a server receipt, verifies `exposure.confirmationRequired`, and confirms the value was applied or rendered. | Later learning and outcome correlation. | `exposure.boardPressure` after `confirmExposure(decisionId, exposure.confirmToken)` |

If runtime strategy evaluation should branch on a value, it must be declared once as an app-emitted metric handle and selected as `inference.inputs`. The application should provide the pre-aggregated value with the request through that handle; the runtime service should not aggregate it on the hot path. Aggregated metrics must be declared as derived signal handles with their source signals and aggregation expression. Evidence views can be derived internally from the definition revision, referenced signal definitions, target hierarchy, and requested windows. Decision records capture returned values; exposure capture is still useful because it records the exact input values present when the application actually applied or rendered a decision.

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

The target hierarchy is the abstraction that prevents separate hard-coded models for async learning, governed state, runtime execution, evidence, policy, and fallback.

```text
session -> user -> cohort -> global
```

Each path resolves a target role from the same hierarchy:

| Target role | Chosen by | Meaning |
| --- | --- | --- |
| Runtime target | Runtime decision execution | Concrete entity receiving the decision now. |
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
- safety/policy envelope changes,
- authority mode, initial authority target, rule, or rationale changes.

Metadata-only changes may keep the same semantic revision if the registry can prove runtime behavior is unchanged.

Definition publication is a control-plane operation independent from application deployment. For MVP, static code-first extraction supplies a canonical bundle to trusted application/bootstrap startup, which validates/applies it, obtains authenticated approval, activates required initial authority, and only then initializes the data-plane binding. Future control-plane clients may publish manually or through CLI, CI/CD, GitOps, deployment hooks, verify-only startup, or registry-first tooling. Decide never registers a definition. If startup publication or required activation fails, Flaggo initialization remains non-ready.

## Tetris example

```text
DecisionDefinition
  key: tetris.dropInterval
  definitionId: def_01JQ8Y7M6X3K9P2W4R5T6V7N8A
  revision: rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3
  contractDigest: sha256:contract...
  targetHierarchy: session -> user -> cohort -> global
  signals:
    allow:
      - tetris.piecePlaced
      - tetris.sessionEnded
      - tetris.boardPressure
      - tetris.recentPlacementTimeMs
      - tetris.recoveryFailures
      - tetris.currentLevel
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
      - tetris.recoveryFailures
      - tetris.currentLevel
    fallbackOrder: cohort -> global
  output:
    type: number
    range: 200..1500
    step: 50
    default: 800
  lifecycle:
    authorityMode: bundle-approved
    initialAuthority:
      controlTarget: cohort:new_players
      kind: numeric-rule
      rule:
        threshold: 0.55
        valueAtOrAbove: 850
        valueBelow: 750
        weightedInputs:
          - signal: tetris.boardPressure
            minimum: 0
            maximum: 1
            weight: 0.45
          - signal: tetris.recentPlacementTimeMs
            minimum: 0
            maximum: 2000
            weight: 0.25
          - signal: tetris.recoveryFailures
            minimum: 0
            maximum: 5
            weight: 0.20
          - signal: tetris.currentLevel
            minimum: 0
            maximum: 20
            weight: 0.10
      rationale: Initial deterministic Tetris behavior.
  policy:
    kind: inline
    constraints:
      - kind: max-delta
        value: 50
```

`initialAuthority` is part of semantic identity but is only a candidate.
Authenticated bundle approval authorizes the exact candidate, and registration
is not ready until the derived state is active.

For revised Phase 3, `max-delta` compares the rule output with the fixed
contract baseline `actionSpace.default = 800`; it does not imply
previous-result or request-time stabilization semantics.

## Design rule

> A decision definition declares the semantic contract and may declare an
> initial authority candidate. It never contains self-approved active state,
> learned replacement authority, or runtime results.
