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
- what bounds and fallback values keep the experience safe,
- which initial runtime rule should become authority after explicit approval.

At runtime, the game asks Flaggo for the current `dropInterval` decision for a
runtime target such as the current session. Flaggo uses the versioned decision
definition, runtime context, approved governed state, and runtime policy to
return a governed value. The game applies the value and emits linked outcomes.

Phase 3 demonstrates real-time contextual adaptation from an explicitly
approved, bundle-authored numeric rule. It does not claim telemetry ingestion,
learning, or asynchronous proposal generation. Phase 4 later closes that loop
without changing the runtime execution path.

The scenario uses the concepts from [Mental Model](MENTAL_MODEL.md), [Decision Definition](DECISION_DEFINITION.md), [Decision Evidence](DECISION_EVIDENCE.md), [Decision Intelligence](DECISION_INTELLIGENCE.md), [Decision Lifecycles](DECISION_LIFECYCLES.md), and [Runtime Decision Execution](RUNTIME_DECISION_EXECUTION.md):

| Category | Tetris example |
|---|---|
| Decision surface/key | `tetris.dropInterval` |
| Decision definition | Opaque registry identity such as `def_01JQ... / rev_01JQ...` |
| Runtime target | `session:game-456` |
| Control target | `cohort:new_players` or `global` |
| Runtime context | `userId`, `sessionId`, `cohort` |
| Inference inputs | `currentLevel`, `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures` |
| Evidence views | hard-drop rate, placement time, early game-over rate by session/cohort/global windows |
| Goals | keep hard-drop rate near target; reduce early losses |
| Action-space constraints | min/max value and `50ms` step |
| Policy constraints | max delta; temporal stabilization is clarified separately |
| Initial authority | bundle-declared numeric rule for `cohort:new_players` |
| Governed state | approved strategy, state identity, generation, predecessor, and approval reference |
| Uncertainty | not claimed for the bundle-authored Phase 3 rule |
| Action space | numeric interval from `200ms` to `1500ms` in `50ms` steps; strategy may further narrow range for a segment |
| Fallback contract | use audited server fallback `800ms` when no compatible authority or policy permits it; optionally configure the same local value only for an eligible data-plane outage |
| Audit/explanation | returned value, rule inputs, approval/activation lineage, policy result |

## The user experience we want

### Developer experience

The developer should not have to build an experimentation platform, telemetry pipeline, metrics aggregation layer, policy engine, contract registry workflow, or decision loop by hand before seeing value.

The primary developer loop should stay small:

```text
declare -> approve -> decide -> observe
```

The TypeScript hero path should ask the developer to express two things:

1. **Declare** the bounded decision definition, including target hierarchy,
   context, safety, fallback, and initial authority candidate.
2. **Decide** by asking for a concrete value with live gameplay context.
   Flaggo links decisions, confirmed exposures, and emitted outcomes through
   instrumentation.

This code-first path is an ergonomic authoring mode, not the only control-plane model. The same decision contract should also be expressible through a language-neutral `flaggo.decision-definition-bundle.json` for bundle-first, registry-first, GitOps, or direct REST-client workflows.

The important experience is that the adaptive value is easy to declare and use in application code, while contract synchronization, definition revisions, runtime target resolution, authenticated bundle approval, strategy activation, policy expansion, and audit linkage remain control-plane concerns.

Example setup intent, not final API:

```ts
const flaggo = await createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  controlPlane: {
    mode: "startup-register",
    bundle: generatedDecisionBundle,
    credential: localBootstrapCredential
  },
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000,
    includeDecisionContext: true
  },
  availabilityFallback: {
    mode: "local-default"
  }
});
```

Startup registration sends the canonical bundle to the control-plane API once and initializes the data-plane client from the accepted receipt. Each production decision request then carries the required definition ID, revision, and contract digest plus optional build/deployment metadata; it does not resend the bundle.

For a bundle-approved definition, the accepted receipt is not complete until
an authenticated actor has approved the exact candidate and the derived state
is active.

The local Tetris MVP may use a trusted local bootstrap host or explicitly insecure local-development control plane. Production browser bundles must not contain management credentials and should use a future backend bootstrap, CLI/CI, deployment hook, or registry-first control-plane client.

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

In this shape, `flaggo.tune.number(...)` keeps the original SDK surface but returns a number decision object. The application still applies a plain numeric value through `dropIntervalDecision.value`, while the SDK exposes the decision receipt needed for attribution. If a value-only convenience is needed later, it should be a separate helper or projection that intentionally opts out of closed-loop exposure attribution.

The code-first object combines authoring and invocation without conflating their persisted forms. A bound input such as `boardPressureSignal.input(boardPressure)` contributes the immutable signal reference to the extracted definition and the current value to the runtime request. A typed target such as `flaggo.target.session(sessionId)` contributes the target kind to the extracted context schema and the current ID to the runtime request. Flaggo excludes bound runtime values from definition digests and revisions.

`signals.evidence` declares emitted or derived signals that this decision may use for evidence and learning; emitting a signal does not associate it with every decision. `inference.target` declares the desired target kind, and `inference.fallbackOrder` keeps resolution explicit. Derived signals such as `earlyLossRateSignal` declare their typed source and aggregation separately. The registry can still govern behavior at a broader control target such as `cohort:new_players`. For a valid registered definition, `output.default` is the governed fallback when evidence, policy, or state prevents an approved adaptive value; contract/configuration errors remain errors.

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

The important design principle is that the decision is declared directly and explicitly. The application does not hide adaptive behavior behind scattered `if/else` branches. It names or references the decision key, output contract, target hierarchy, safety policy, and initial authority candidate. The control plane validates the declaration, records authenticated approval, activates governed state, and preserves audit linkage.

The developer still applies one value:

```ts
gameEngine.updateConfig({ dropInterval: interval });
```

Flaggo produces that value by executing the approved bundle-authored strategy:

```text
current value = 800ms
board pressure = high
recent placement time = slow
recovery failures = 2
approved strategy = slow down by one step when pressure is high and recovery is poor

returned value = 850ms
```

This keeps the game code simple while allowing runtime behavior to adapt to the current session.

### Proposal-managed evidence and governance mode

Phase 4 adds evidence-backed proposal generation and independent governance:

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
    lifecycle: {
      authorityMode: "proposal-managed"
    },
    policy: {
      kind: "inline",
      constraints: [
        { kind: "max-delta", value: 50 },
        { kind: "min-sample-size", value: 30 },
        { kind: "min-evidence-quality", value: 0.7 },
        { kind: "max-model-uncertainty", value: 0.35 }
      ]
    },
    context: {
      sessionId: { type: "string", target: "session" },
      userId: { type: "string", target: "user" },
      cohort: { type: "string", target: "cohort" }
    }
  },
  context: {
    sessionId,
    userId,
    cohort: playerCohort
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

In `bundle-approved` mode, the bundle declares an initial authority candidate
but cannot declare it approved. Flaggo owns the generated proposal and
activation identities, materialized strategy identity, active state,
predecessor, generation, approval reference, and future replacement
transitions.

In `proposal-managed` mode, the bundle contains no initial authority. A new or
semantically changed definition still requires authenticated approval of the
exact bundle snapshot before its runtime revision is published. Because that
workflow has no bundle-derived initial authority, approved definition
publication can produce the ready registration receipt without an activation
step. Until a future Phase 4 producer and governance path activate authority,
runtime requests use the registered audited fallback. The concrete proposal
and governance DTOs remain deferred to issue #25.

Future governance may activate initial authority when no governed state exists
or replacement authority against the current state head. The first activation
reuses the shared state boundary with the no-state expected baseline
(generation `0` and no `stateId`) and creates a state with no predecessor.
This is a lifecycle invariant, not a concrete Phase 4 DTO design.

The declaration can produce or contribute to a canonical contract bundle during build or release:

```text
bundle-approved definition
  -> validate and apply bundle
  -> authenticated approval of the exact snapshot
  -> initial authority activation
  -> ready registration receipt with activated-authority references

proposal-managed definition
  -> validate and apply bundle
  -> authenticated approval of the exact snapshot
  -> publish definition without initial authority activation
  -> registration receipt without activated-authority references
  -> future proposal governance may activate initial or replacement authority

both
  -> each deployed workload carries its exact expected contract/build identity
```

### Software lifecycle experience

Flaggo should support different owners and systems across the software lifecycle. The SDK is important in development and runtime, but it should not be the only way to synchronize contracts. Application deployment is independent from the Flaggo control plane.

| Stage | Developer or platform action | Flaggo artifact | SDK/runtime role |
| --- | --- | --- | --- |
| Development | Author decision declaration in TypeScript, JSON/YAML, or registry UI. | Local declaration or draft contract bundle. | SDK provides ergonomic code-first declarations and typed runtime calls. |
| Build | Optionally extract or assemble a canonical contract bundle. | `flaggo.decision-definition-bundle.json`, `contractDigest`, optional build metadata. | SDK extractor may generate the bundle; bundle-first and registry-first workflows remain valid. |
| Application deployment | Deploy application code independently. | Extracted bundle may be packaged for trusted startup. | Flaggo does not own or block external deployment. |
| Application/bootstrap startup | MVP validates and applies the bundle. Every new or semantically changed definition requires authenticated approval. Bundle-approved definitions then wait for required initial activation; proposal-managed definitions become ready after approved publication because they declare no initial authority. | Registration receipt with exact definition identity and activated-authority references only when bundle-approved initial authority exists. | Trusted startup SDK is the initial control-plane client; it uses management APIs, never the decide endpoint. |
| Runtime | Ask for decisions and emit telemetry. | Request with exact expected identity; strict server result or Problem Details error. | Data plane evaluates only registered identities. Missing/conflicting identity is surfaced without local fallback; availability fallback remains explicitly configurable. |
| Observe/operate | Inspect drift, audit, fallback, and strategy behavior. | Audit records, diagnostics, integrity metrics, operator warnings. | SDK exposes response fields; control plane owns audit, strategy, policy, and operator actions. |

This lifecycle supports TypeScript-first development without making CI/CD
integration a requirement. For MVP, trusted startup submits the extracted
bundle and initializes runtime bindings only after required definition approval
and any initial activation complete. Future clients can move that operation to
CLI, CI/CD, GitOps, deployment hooks, verify-only startup, or registry-first
tooling. Code can deploy even when startup registration later fails, but its
Polari calls remain disabled. Rolling deployments remain safe because each
successful startup receives and uses its exact immutable identity.

When a definition changes semantically, Flaggo replaces authority through the
same stable application/environment/decision-key/control-target head. The new
immutable state remains bound to the new exact definition identity, while
telemetry facts and matching immutable signal keys may still be reused:

```text
raw observations: reusable when immutable signal keys match
evidence views: reusable when signal key, target, window, and filters match
authority head: stable across semantic revisions for ordered CAS replacement
governed state record: bound to one exact definition identity
future temporal state: isolated by its explicitly approved address contract
```

This lets a new `dropInterval` revision add a signal such as
`recoveryFailures` while reusing historical `boardPressure` and
`placementTimeMs` evidence. The new signal warms up independently, and the new
strategy becomes authority only through its own approved activation of the
stable head.

### Operator and product experience

The operator should be able to inspect and govern the decision without reading application code.

For `tetris.dropInterval`, the operator should see:

- the decision key: `tetris.dropInterval`,
- the stable decision key plus complete active runtime identity:
  `definitionId`, opaque `revision`, and `contractDigest`,
- the runtime targets receiving decisions, such as sessions,
- the control targets where behavior is governed, such as `cohort:new_players`,
- the declared goal: keep gameplay challenging but playable,
- the action space: `200ms` to `1500ms` in `50ms` steps,
- the fallback contract: `800ms`,
- the enforced action-space bounds/step and active Phase 3 policy constraints,
  including max delta and any declared guardrails,
- the current governed state when one exists: state ID, generation, active
  authority, predecessor, and approval reference,
- the explicit no-authority fallback status before first activation,
- recent decisions and explanations,
- the active decision strategy when one exists and why its exact snapshot was
  approved.

Phase 4 adds evidence quality, model uncertainty, proposal inspection, and
operator actions such as approve, reject, or replace. Pause, override, rollback,
and temporal stabilization require their own approved contracts.

The operator experience matters because Flaggo is not just a metric optimizer. It is a policy-controlled runtime decisioning layer. Human intent must remain visible in goals, boundaries, and operating mode.

### End-user experience

The player should not experience random or chaotic changes. The game should feel like it is adapting thoughtfully:

- if the game is too slow, pieces may fall faster over time,
- if the game is too punishing, pieces may fall slower,
- if policies block adaptation, the player should receive the safe fallback behavior.

For example:

```text
New session starts:
  return 750ms or 850ms from the approved rule

Player is near the top of the board and placing pieces slowly:
  return 850ms

Player stabilizes after recovery:
  return 750ms

No compatible authority exists, or policy rejects the candidate:
  return the audited server fallback of 800ms

The ready data plane is unavailable and SDK availability fallback is configured:
  return the client fallback of 800ms without server audit or exposure identity

Required state or policy dependencies are non-ready:
  return a typed fallback-ineligible error, not a decision value
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

### Runtime context and telemetry evidence have distinct roles

The revised Phase 3 decision uses live runtime context:

- **Runtime context**: user, session, level, device type, segment, environment.

Telemetry such as hard-drop rate, placement time, game duration, early losses,
and configuration changes is emitted and linked for observation. Phase 4 may
turn that telemetry into evidence for replacement proposals. The Phase 3 rule
does not consume learned evidence or claim uncertainty.

### Policies and constraints are first-class

The decision is never just "whatever the model thinks is best." It is bounded by explicit constraints:

- action-space bounds: minimum and maximum drop interval,
- action-space granularity: step size,
- maximum delta from the fixed contract baseline,
- deterministic fallback behavior.

Proposal-managed authority may additionally require evidence quality, model
uncertainty, expected outcome, sample size, and authorized automatic or human
approval. Temporal constraints such as cooldown are specified separately.

Policy is not an afterthought. It is part of the decision contract.

### Uncertainty is acknowledged instead of ignored

Proposal-managed intelligence should not pretend every recommendation is
equally reliable. Bundle-approved authority makes no learned-confidence claim.

For each proposal-managed decision that claims evidence-backed confidence, the
system should expose:

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

Every returned Phase 3 value should be explainable after the fact:

```text
Decision: tetris.dropInterval
Runtime target: session:game-456
Control target: cohort:new_players
Returned value: 850
Reason: weighted runtime score met the approved 0.55 threshold
Authority: bundle-approved
Approval reference: approval_01...
Policy result: approved
Fallback used: no
```

Auditability means the team can reconstruct what happened, why it happened,
which authority and runtime inputs produced it, which policy allowed it, and,
when applicable, what evidence supported a proposal.

### Human intent remains encoded in goals and boundaries

The developer and operator do not ask Flaggo to "make the game better" in an open-ended way.

They encode intent:

- approve a specific initial runtime rule and rationale,
- preserve playable bounds,
- require safe fallback behavior.

Phase 4 may additionally encode metric objectives such as reducing early losses
or keeping hard-drop rate near a target.

AI-native decisioning should amplify human intent, not replace it.

### Fallback behavior exists when safe runtime execution is unavailable

The game must always have a safe behavior even when Flaggo cannot decide.

For a valid registered Phase 3 request, Flaggo returns the audited server
fallback when:

- no permitted target has compatible active state,
- runtime policy blocks the candidate.

Separately, an explicitly configured SDK availability fallback may return the
same value when the data plane is unavailable or times out. It has client
provenance and cannot claim a server decision, policy, audit, or exposure.

Missing or invalid inference inputs or runtime context are request errors.
Missing, conflicting, retired, or non-ready contract identity is a contract or
initialization error. These errors use Problem Details or failed initialization
and never become server or client fallback decisions.

Corrupt, torn, or internally incoherent persisted state also fails service
readiness; it is not treated as an ordinary absence of compatible authority.

Proposal-managed authority may add evidence, uncertainty, expected-outcome, or
operator-mode fallback reasons.

For this scenario, fallback is simple:

```text
tetris.dropInterval -> 800ms
```

Both fallback paths return `800ms`, but their provenance remains distinct.
Fallback is part of the primitive, not an exception path left to each developer
to rediscover.

## Desired runtime loop

```text
Developer declares a decision key, target, action space, initial authority, safety policy, and fallback
        ↓
Trusted control-plane client obtains approval and active state
        ↓
Application asks Flaggo for runtime decision with runtime context
        ↓
Flaggo evaluates runtime context, governed state, and runtime policy
        ↓
Flaggo returns value + explanation + audit record
        ↓
Application applies value or fallback
        ↓
Telemetry records outcome
```

Phase 4 adds the separate evidence-to-proposal loop that may replace authority
through the same activation boundary.

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

For the Tetris scenario, that means the first product slice should make
`tetris.dropInterval` explicit as a stable decision key with a versioned
definition and bundle-approved initial rule, driven by declared runtime inputs,
bounded by action space and policy, activated as governed state, explainable
through approval and runtime audit context, and safe by fallback contract.
Phase 4 adds reusable evidence, uncertainty, and independent replacement
proposals.
