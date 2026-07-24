# Flaggo High-Level Design

## Purpose

This document describes the high-level design for Flaggo: an AI-native runtime decisioning system that fulfills the vision in [MANIFESTO.md](MANIFESTO.md) and supports the first product experience in [HERO_SCENARIO.md](HERO_SCENARIO.md).

This is intentionally not a detailed component specification. Specific API, SDK, storage, policy, telemetry, frontend, and agent designs should live in focused sub-documents later.

## Product thesis

Flaggo gives running software a governed way to ask:

> Given this decision surface, this scope, current runtime context, available evidence, goals, policies, system state, uncertainty, action space, and fallback contract, what should happen now?

The application still owns execution. Flaggo owns the decisioning control plane around selected runtime choices.

## Core concepts

### Decision surface

A decision surface is the explicit place where application code delegates a runtime choice to flaggo. It answers: **what is being decided?**

Examples include `tetris.dropInterval`, `checkout.fraudReviewRequired`, `api.retryPolicy`, `llm.modelRoute`, and `workflow.escalationAction`.

Detailed concept design: [DECISION_SURFACE.md](DECISION_SURFACE.md).

### Decision scope

Decision scope is the boundary at which a decision is evaluated, applied, measured, governed, and remembered.

It answers: **for whom, where, or at what aggregation boundary is this decision being made?**

Scope is the coordinating concept that resolves applicable runtime context, telemetry evidence, goals, policy constraints, state, uncertainty, action space, and fallback behavior.

Detailed concept design: [DECISION_SCOPE.md](DECISION_SCOPE.md).

### Decision factors

Decision factors are the inputs and constraints that shape a governed decision. They are described in detail in [DECISION_FACTORS.md](DECISION_FACTORS.md).

At the high level:

```text
decision surface + decision scope
  -> resolve runtime context
  -> resolve telemetry evidence
  -> resolve goals
  -> resolve policy constraints
  -> resolve system state
  -> assess uncertainty
  -> select from action space
  -> return decision or fallback
```

### Governed decision

A governed decision is the output Flaggo returns to application code.

It should include:

- selected value or action,
- whether fallback was used,
- confidence/evidence status,
- policy result,
- explanation summary,
- audit correlation ID.

The application should be able to apply the result without knowing the internals of evidence aggregation, policy evaluation, or AI reasoning.

## Hero scenario design target

The first complete design target is the Tetris `dropInterval` decision.

```text
Decision surface: tetris.dropInterval
Decision scope: user or session, with fallback to segment/global
Runtime context: userId, sessionId, currentLevel, deviceType
Telemetry evidence: hard-drop rate, placement time, early game-over rate
Goals: keep gameplay challenging but playable
Policy constraints: min/max interval, max delta, cooldown, confidence floor
System state: active value, previous value, cooldown, operator mode
Action space: numeric interval from 200ms to 1500ms
Fallback contract: 800ms default
```

This scenario should prove the smallest useful version of Flaggo:

1. A developer can declare telemetry and a decision surface.
2. The app can emit evidence and ask for a scoped decision.
3. Flaggo can evaluate evidence, goals, policy, state, and uncertainty.
4. The app can safely apply a value or fallback.
5. An operator can inspect why the decision happened.

## System components

### 1. Client library

The client library is the developer-facing integration point.

Responsibilities:

- define decision surfaces,
- define action spaces and fallbacks,
- define or emit domain telemetry,
- pass runtime context when asking for decisions,
- receive decision responses,
- apply fallback behavior when Flaggo is unavailable or blocks a decision,
- integrate with OpenTelemetry where configured.

The client library should make the runtime primitive feel natural:

```text
define events -> define evidence metrics -> declare decision -> ask decision -> apply result
```

For the Tetris hero scenario, the TypeScript client is the first likely library target.

### 2. Decision API service

The Decision API is the runtime service applications call when they need a governed decision.

Responsibilities:

- receive decision requests,
- validate decision surface and scope,
- resolve applicable contracts and factor configuration,
- fetch evidence and system state,
- evaluate policy constraints,
- invoke decision reasoning or deterministic selection logic,
- return a value/action, explanation, confidence, policy result, and fallback status.

The Decision API must be fast, reliable, and safe-by-default. If it cannot decide safely, it should return fallback guidance rather than pretending confidence exists.

### 3. Telemetry and evidence service

The telemetry/evidence service turns runtime observations into decision evidence.

Responsibilities:

- ingest domain events and OpenTelemetry-compatible signals,
- correlate events, metrics, traces, logs, and decision records,
- support local/context-scoped evidence and server-side aggregation,
- expose evidence snapshots to the Decision API,
- preserve enough evidence lineage for explanations and audits.

The service should support both direct Flaggo ingestion and integration through OpenTelemetry pipelines.

### 4. Contract and registry service

The contract/registry service stores the declared meaning of decisions.

Responsibilities:

- register decision surfaces,
- store action spaces,
- store fallback contracts,
- store goal definitions,
- store telemetry/evidence definitions,
- store scope rules and resolution chains,
- version contract changes.

Contracts should be explicit and versioned because runtime decisions must be explainable after the fact.

### 5. Policy service

The policy service is the safety gate.

Responsibilities:

- resolve applicable policies by decision surface and scope,
- enforce hard constraints,
- evaluate confidence floors, sample-size requirements, cooldowns, max deltas, approval requirements, and guardrails,
- block or require fallback when safety requirements are not met,
- produce stable reason codes for audit and operator visibility.

Policy is not advisory. It is part of the control plane.

### 6. State service

The state service tracks the current and historical state of decisions.

Responsibilities:

- active value/action per surface and scope,
- previous decisions,
- cooldown state,
- rollout or exposure state,
- operator overrides,
- paused/resumed mode,
- rollback state.

State lets Flaggo avoid stateless one-off guesses and prevents thrashing or conflicting decisions.

### 7. Decision reasoning engine

The decision reasoning engine proposes or selects the next safe action.

Responsibilities:

- interpret evidence relative to goals,
- account for uncertainty,
- compare candidate actions within the action space,
- produce a rationale,
- hand candidate decisions to policy before application.

This engine may use AI, deterministic algorithms, statistical methods, bandits, rules, or hybrids. The design should not assume every decision requires an LLM.

### 8. Audit and explanation service

The audit/explanation service records why decisions happened.

Responsibilities:

- persist decision request/response summaries,
- record scope, evidence snapshot, policy result, confidence, fallback usage, and reason text,
- correlate decisions with telemetry and traces,
- support operator review and debugging.

Auditability is required for trust. It is not optional observability.

### 9. Operator console

The operator console is the human governance surface.

Responsibilities:

- list decision surfaces,
- inspect scopes and resolution chains,
- view goals, policies, fallbacks, and active state,
- inspect recent decisions and explanations,
- pause/resume decisions,
- override values,
- approve, reject, or roll back changes,
- observe evidence quality and uncertainty.

The console should make Flaggo feel governed rather than magical.

## High-level runtime flow

```text
Application code
  -> emits domain telemetry through client library
  -> asks Decision API for decision(surface, scope, runtime context)

Decision API
  -> loads decision contract
  -> resolves scope chain
  -> fetches telemetry evidence
  -> fetches system state
  -> evaluates goals and uncertainty
  -> asks reasoning engine for candidate action
  -> applies policy gate
  -> records audit/explanation
  -> returns decision or fallback

Application code
  -> applies returned value/action
  -> emits outcome telemetry
```

## Scope resolution model

Scope resolution is central to flaggo.

For a request like:

```text
surface = tetris.dropInterval
scope = user:123
```

Flaggo may resolve factors through:

```text
user:123 -> segment:new_players -> global
```

That means:

- telemetry evidence may include user/session evidence and segment evidence,
- policy may be user-specific or fall back to global,
- goals may be inherited from global unless overridden,
- state may be user-specific,
- fallback may use a user override or global default.

This is what lets Flaggo support both personalization and broader operational decisions without changing the core model.

## Initial boundaries

The first design should stay narrow:

- one hero decision: `tetris.dropInterval`,
- one client library: TypeScript,
- one default Decision API,
- one telemetry/evidence path,
- one policy gate,
- one audit trail,
- one basic operator view.

The goal is not to implement every scenario. The goal is to prove the primitive end-to-end.

## Explicit non-goals for the first design

- Full replacement for feature flag platforms.
- Full replacement for observability platforms.
- General-purpose workflow orchestration.
- Unbounded autonomous code execution.
- LLM-only decisioning.
- Complex multi-tenant enterprise governance from day one.

## Design principles

1. **Explicit over implicit**: decisions, scopes, goals, policies, action spaces, and fallbacks must be named.
2. **Governed over magical**: policy and fallback are first-class.
3. **Evidence-aware over telemetry-blind**: decisions must be grounded in runtime context and observed behavior.
4. **Uncertainty-aware over false precision**: weak evidence should block, suggest, or fall back.
5. **Scoped over global-by-default**: every decision should know where it applies.
6. **Auditable over opaque**: every decision should be reconstructable.
7. **Pluggable over closed**: integrate with OpenTelemetry, existing config systems, and future decision engines.

## Future focused design documents

Later sub-documents should define:

- client SDK design,
- decision request/response API,
- decision contract schema,
- scope and resolution rules,
- telemetry/evidence model,
- policy model,
- state model,
- audit/explanation model,
- operator console UX,
- Tetris integration design.
