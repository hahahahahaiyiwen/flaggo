# Decision Surface

## Purpose

A decision surface is the explicit place where application code delegates a runtime choice to Polari.

It answers:

> What is being decided?

Decision surfaces make adaptive behavior visible. Instead of hiding dynamic behavior in scattered `if/else` branches, flags, configuration lookups, or local heuristics, the application names the runtime choice and declares the shape of the result it expects.

## Definition

A decision surface is a named runtime decision interface.

Examples:

- `tetris.dropInterval`
- `checkout.fraudReviewRequired`
- `api.retryPolicy`
- `llm.modelRoute`
- `workflow.escalationAction`

A surface should describe the decision, not the implementation mechanism. For example, `tetris.dropInterval` is better than `callPolariForGameSpeed`, because the former names the runtime choice and the latter names an implementation detail.

## What a decision surface owns

A decision surface should define or reference:

- decision name,
- result type,
- action space,
- fallback contract,
- applicable scope hierarchy,
- runtime context shape,
- telemetry/evidence definitions,
- goal definitions,
- policy constraints,
- audit/explanation expectations.

The surface is the anchor that connects all other decision concepts.

## What a decision surface does not own

A decision surface should not directly own:

- the current active value,
- rollout state,
- cooldown state,
- operator override state,
- historical evidence,
- telemetry storage,
- model internals.

Those belong to Polari services such as state, telemetry/evidence, policy, and audit.

## Naming guidance

Decision surface names should be stable, domain-oriented, and readable.

Recommended pattern:

```text
<domain>.<decision>
```

Examples:

```text
tetris.dropInterval
checkout.fraudReviewRequired
api.retryPolicy
llm.modelRoute
```

Avoid names that encode implementation details:

```text
bad: useAiForDropSpeed
bad: flag_tetris_speed_v2
bad: getRemoteValueForInterval
```

## Result shape

A decision surface should make the expected result shape explicit.

The initial Polari design should keep result shapes intentionally small:

- **boolean**: choose true or false,
- **number**: choose a bounded numeric value,
- **string**: choose a textual value, usually from an allowed set.

The result shape defines what the application can safely apply.

This small set is enough for many richer decisions:

- branch decisions can use boolean,
- thresholds and tuning parameters can use number,
- variants, routes, model names, workflow actions, and strategy names can use string.

For example:

```text
checkout.fraudReviewRequired -> boolean
tetris.dropInterval -> number
llm.modelRoute -> string
api.retryStrategy -> string
workflow.nextAction -> string
```

Numbers should define numeric constraints:

```text
type: number
min: 200
max: 1500
step: 50
default: 800
```

Strings may define allowed values when the decision must choose from a known set:

```text
type: string
allowedValues:
  - gpt-4.1-mini
  - gpt-4.1
  - local-small-model
default: gpt-4.1-mini
```

Polari should avoid structured-object results in the first design. If a complex result is needed, the developer can represent a known configuration or action by string and resolve it in application code. This keeps the decision surface stable, auditable, and easy to validate.

## Relationship to application code

Application code owns execution. Polari owns decision support.

The code should call a decision surface, receive a governed result, and then execute application behavior:

```text
application code -> asks decision surface -> receives governed decision -> applies result
```

The surface should make adaptive behavior discoverable:

```text
What decisions does this application delegate to Polari?
```

That question should be answerable from registered decision surfaces.

## Tetris example

For the hero scenario:

```text
Decision surface: tetris.dropInterval
Result type: number
Action space: 200ms to 1500ms in 50ms steps
Fallback: 800ms
Runtime context: userId, sessionId, currentLevel, deviceType
Telemetry evidence: hard-drop rate, placement time, early-loss rate
Goal: keep gameplay challenging but playable
```

The application asks:

```text
What drop interval should this game session use now?
```

Polari answers with a governed numeric value and supporting explanation.

## Design rule

> A decision surface is the stable contract between application execution and Polari decisioning.

Everything else can evolve behind that surface: evidence sources, policies, models, state, and operator controls.
