# Decision Scope

## Purpose

Decision scope defines the boundary at which a decision is evaluated, applied, measured, governed, and remembered.

It answers:

> For whom, where, or at what aggregation boundary is this decision being made?

Scope is a coordinating concept. It binds runtime context, telemetry evidence, goals, policy constraints, system state, uncertainty, action space, fallback behavior, and audit records to the same decision boundary.

## Core model

Decisioning starts with two coordinates:

```text
decision surface + decision scope
```

Example:

```text
surface = tetris.dropInterval
scope = user:123
```

Polari then resolves the applicable factors for that surface and scope.

## Minimal built-in primitives

Polari should standardize scope mechanics, not dictate every domain hierarchy.

The built-in scope primitives should stay intentionally small:

- `global`
- `user:{id}`
- `session:{id}`
- `segment:{name}`
- `custom:{type}:{id}`

These primitives support the hero scenario while allowing advanced users to represent service, endpoint, tenant, workflow, model, queue, region, store, or other domain-specific scopes through `custom`.

## User-defined hierarchy

Users define the scope hierarchy per decision surface.

Examples:

```text
tetris.dropInterval:
  session -> user -> segment -> global

api.retryPolicy:
  custom:endpoint -> custom:service -> global

llm.modelRoute:
  custom:tenant -> custom:workflow -> global
```

The hierarchy defines how Polari resolves inherited or fallback factor configuration.

## Ownership model

Polari owns:

- scope primitive format,
- resolution mechanics,
- fallback and inheritance semantics,
- validation shape,
- audit representation.

Users own:

- which scopes matter for each decision surface,
- hierarchy order,
- how runtime context maps to scope keys,
- which factors may override or inherit at each scope.

## Scope resolution

Scope resolution determines which factor definition applies.

For example:

```text
surface = tetris.dropInterval
requested scope = session:abc
resolution chain = session:abc -> user:123 -> segment:new_players -> global
```

Polari may resolve:

- user/session telemetry evidence,
- segment-level fallback evidence,
- global default goals,
- user-specific or global policy,
- session-specific active state,
- global fallback value.

Scope resolution lets Polari support personalization and broader operational decisions with the same conceptual model.

## Relationship to runtime context

Runtime context is the current fact snapshot passed at decision time.

Scope is derived from, or selected using, runtime context.

Example:

```text
runtime context:
  userId = 123
  sessionId = abc
  deviceType = mobile

resolved scopes:
  session:abc
  user:123
  segment:new_players
  global
```

Runtime context is not the same as scope. Runtime context is input data. Scope is the boundary used to resolve decision factors.

## Relationship to telemetry evidence

Telemetry evidence should be scoped.

Examples:

```text
sessionHardDropRate: scope=session:abc
userHardDropRate: scope=user:123
newPlayerHardDropRate: scope=segment:new_players
globalHardDropRate: scope=global
```

Different scopes can produce different evidence. If local/session evidence conflicts with segment/global evidence, Polari should treat that as uncertainty rather than blindly choosing one source.

## Relationship to policy and fallback

Policy and fallback can also be scoped.

Example:

```text
user:123 policy -> not configured
segment:new_players policy -> maxDelta = 50ms
global policy -> minConfidence = 0.7
```

Fallback can resolve similarly:

```text
session fallback -> none
user fallback -> none
segment fallback -> 900ms
global fallback -> 800ms
```

The design should make this resolution explicit and auditable.

## Tetris example

For `tetris.dropInterval`, the likely first hierarchy is:

```text
session -> user -> segment -> global
```

This lets Polari decide at a user/session boundary while falling back to broader evidence and policy when local evidence is weak.

Example:

```text
surface: tetris.dropInterval
requested scope: session:game-456
runtime context:
  userId: user-123
  sessionId: game-456
  currentLevel: 3
  deviceType: mobile

resolution:
  session:game-456
  user:user-123
  segment:new_players
  global
```

## Design rule

> Polari should standardize scope mechanics, while users define scope hierarchy and domain meaning.

This keeps the core system simple while allowing advanced domains to extend through `custom`.
