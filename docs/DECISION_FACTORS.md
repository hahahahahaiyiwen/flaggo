# Decision Factors for AI-Native Runtime Decisioning

## Purpose

Flaggo needs a clear vocabulary for what drives a runtime decision. Without this, terms like "context", "telemetry", "policy", and "intent" can collapse into one vague bucket.

This document defines the main categories of decision-driving factors for AI-native runtime decisioning.

## Core idea

A governed runtime decision is not produced from telemetry alone. It is produced from a combination of current facts, observed evidence, allowed actions, human intent, safety constraints, system state, and fallback rules.

```text
decision surface
+ runtime context
+ telemetry evidence
+ goals
+ policy constraints
+ system state
+ uncertainty
+ action space
+ fallback contract
=> governed runtime decision
```

## 1. Decision surface

The decision surface is the place where software delegates a runtime choice to flaggo.

It answers:

> What is being decided?

Examples:

- `tetris.dropInterval`
- `checkout.fraudReviewRequired`
- `search.rankingStrategy`
- `api.retryPolicy`
- `llm.modelRoute`
- `workflow.escalationAction`

The decision surface is not runtime context. It is the explicit integration point where application code asks for a decision.

Application code belongs here conceptually: it defines where the decision is requested, what result shape is expected, and how the result is applied.

## 2. Runtime context

Runtime context is the snapshot of current facts available at decision time.

It answers:

> What is true about this request, user, entity, environment, or workflow right now?

Examples:

- user ID,
- session ID,
- tenant,
- segment,
- region,
- device type,
- current game level,
- request type,
- environment,
- plan tier,
- workflow state,
- current model,
- current configuration value.

Runtime context is usually passed by the application when it asks for a decision:

```text
dropInterval.decide({
  userId,
  sessionId,
  currentLevel,
  deviceType
})
```

Runtime context should be treated carefully. Some context is useful for decisioning but dangerous as metric dimensions because it can create high cardinality or privacy risk.

## 3. Telemetry evidence

Telemetry evidence is observed behavior collected over time.

It answers:

> What has happened, and what do recent or historical measurements suggest?

Examples:

- hard-drop rate,
- average placement time,
- early game-over rate,
- request latency p95,
- error rate,
- conversion rate,
- retry success rate,
- model cost per request,
- guardrail violation rate.

Telemetry evidence can come from events, metrics, traces, logs, or derived aggregates.

Important distinction:

```text
runtime context: this request is from user 123 on mobile
telemetry evidence: mobile users had a 28% early-loss rate over the last hour
```

### Evidence from different OpenTelemetry signals

Flaggo should treat OpenTelemetry as an evidence source, not as a single uniform data shape. Different OTel signals answer different decision questions.

| Signal | What it contributes | Decisioning considerations |
|---|---|---|
| **Metrics** | Aggregated numeric behavior over time. | Best for rates, averages, percentiles, counts, guardrail thresholds, and trend checks. Metrics are first-class OTel signals, but high-cardinality dimensions such as raw `userId` should be used carefully. |
| **Traces** | Request or workflow path across components. | Useful for decisions inside distributed flows, latency attribution, downstream dependency behavior, and correlating a decision with what happened before and after it. |
| **Span events** | Discrete facts attached to a traced operation. | Useful when a domain event matters within a request or workflow, such as "decision requested", "fallback used", or "hard drop pressed during game span". |
| **Logs** | Timestamped structured records. | Useful for domain events, debugging, and audit correlation. Logs are flexible but can be noisy and expensive if used as the only evidence source. |
| **Baggage/context propagation** | Cross-service contextual metadata. | Useful for propagating decision IDs, tenant, experiment, or scope, but should avoid sensitive or high-cardinality values unless intentionally governed. |

Flaggo can expose a developer-facing concept like **domain event** even if OpenTelemetry represents it as a structured log, span event, or metric input. The product abstraction should be domain-friendly while remaining compatible with OTel transport and correlation models.

### Local evidence vs decision-time aggregation

Flaggo needs to support multiple evidence scopes because not every decision should be based on the same aggregation boundary.

#### Local/context-scoped evidence

Some decisions need evidence scoped to a single user, session, browser, game instance, device, request, or workflow.

Tetris is a good example. A frontend may know that this specific player recently:

- pressed hard drop frequently,
- placed pieces quickly,
- lost early,
- changed behavior after a drop-speed update.

This can produce local decision evidence such as:

```text
sessionHardDropRate over last 2 minutes
currentGamePlacementTimeAverage
currentSessionEarlyLossIndicator
```

Local evidence is useful when:

- the decision should personalize to one user/session,
- immediate feedback matters,
- raw events are high-volume,
- offline or degraded operation matters,
- server-side aggregation has not caught up yet.

Local evidence also has risks:

- small sample sizes,
- noisy short windows,
- incomplete event history,
- client trust and tampering concerns,
- overfitting to a single session.

Flaggo should treat local evidence as useful but uncertainty-bearing.

#### Decision-time/server aggregation

Other decisions need aggregation across many instances, users, tenants, services, or time windows.

Examples:

```text
hardDropRate across new players in the last hour
p95 API latency across all service instances
conversion rate by segment
model cost per tenant over 24 hours
guardrail violation rate across all requests
```

Decision-time aggregation is useful when:

- evidence must span many application instances,
- the decision affects a segment, tenant, or global setting,
- policy requires sample-size and freshness checks,
- the system needs cohort comparisons,
- local context is insufficient or biased.

Flaggo should be able to compute or retrieve decision-time evidence from ingested events, OpenTelemetry metric streams, traces, logs, observability queries, or its own evidence store.

#### Design implication

Telemetry evidence should carry an explicit scope:

```text
evidence scope = local | request | user | session | segment | tenant | service | global | custom
```

For example:

```text
sessionHardDropRate: scope=session, window=2m
newPlayerHardDropRate: scope=segment:new_players, window=1h
serviceLatencyP95: scope=service, window=10m
```

The important design rule:

> Developers define what evidence means. Flaggo determines whether that evidence comes from local observations, OpenTelemetry aggregation, decision-time server queries, or a combination of sources.

## 4. Goals and human intent

Goals encode what humans want the system to improve, preserve, or avoid.

They answer:

> What does "better" mean for this decision?

Examples:

- keep hard-drop rate near 45%,
- minimize early game-over rate,
- reduce p95 latency,
- maximize successful task completion,
- minimize cost while preserving quality,
- reduce manual review volume without increasing fraud loss,
- prefer stability over frequent changes.

Goals are not the same as policies. Goals describe desired direction. Policies define hard boundaries and safety rules.

For Flaggo, human intent should remain encoded in goals and boundaries. The system should not infer open-ended intent like "make the game better" without explicit objective definitions.

## 5. Policy constraints

Policy constraints are rules that govern what Flaggo is allowed to do.

They answer:

> What must never be violated, even if a metric could improve?

Examples:

- `dropInterval` must stay between `200ms` and `1500ms`,
- max change per decision is `50ms`,
- confidence must be at least `0.7`,
- sample size must be at least `100`,
- do not change more often than once per 5 minutes,
- block rollout if guardrail score is below threshold,
- require human approval for production mode,
- comply with tenant-specific restrictions.

Policies are first-class decision inputs. They should be deterministic, explainable, and auditable.

## 6. System state

System state is the current state of Flaggo and the controlled decision.

It answers:

> What is already happening in the control plane?

Examples:

- current active value,
- previous decision,
- active rollout,
- paused/resumed mode,
- observe-only mode,
- operator override,
- cooldown state,
- in-flight decision,
- rollback status,
- last successful decision time,
- last policy block reason.

System state prevents decisions from being stateless one-off guesses. It helps the system avoid thrashing, conflicting changes, and unsafe repeated actions.

## 7. Uncertainty and evidence quality

Uncertainty describes how reliable the decision evidence is.

It answers:

> How much should Flaggo trust the current evidence?

Examples:

- confidence score,
- sample size,
- data freshness,
- missing telemetry,
- conflicting metrics,
- noisy measurements,
- outlier-sensitive evidence,
- model disagreement,
- cold-start state,
- segment drift.

Uncertainty should not be hidden. It should influence whether Flaggo applies, suggests, blocks, or falls back.

## 8. Action space

The action space defines what Flaggo is allowed to return.

It answers:

> What choices are possible?

Examples:

- boolean: enter branch or do not enter branch,
- number: choose `dropInterval`,
- enum: choose ranking strategy,
- route: choose model or service endpoint,
- threshold: choose risk cutoff,
- policy value: choose retry count or timeout,
- workflow action: continue, pause, escalate, degrade.

The action space should be explicit and bounded. AI-native decisioning should not mean arbitrary action generation.

## 9. Fallback contract

The fallback contract defines what happens when Flaggo cannot safely decide.

It answers:

> What should the application do if decisioning is unavailable, unsafe, or uncertain?

Examples:

- use default value,
- use previous safe value,
- use static branch,
- deny high-risk action,
- require human review,
- preserve current configuration,
- degrade gracefully.

Fallback is part of the decision primitive. It should not be an afterthought left to every call site.

## 10. Audit and explanation context

Audit and explanation context captures why a decision happened.

It answers:

> Can we reconstruct the decision later?

Examples:

- decision ID,
- timestamp,
- scope,
- input context,
- evidence snapshot,
- metric values,
- policy result,
- confidence,
- returned value,
- fallback usage,
- operator mode,
- reason text.

This category does not necessarily drive the decision directly, but it is required for trust, debugging, governance, and iteration.

## Tetris example

For the Tetris hero scenario:

| Category | Example |
|---|---|
| Decision surface | `tetris.dropInterval` |
| Runtime context | `userId`, `sessionId`, `currentLevel`, `deviceType` |
| Telemetry evidence | hard-drop rate, placement time, early game-over rate |
| Goals | keep hard-drop rate near target; reduce early losses |
| Policy constraints | min `200ms`, max `1500ms`, max delta `50ms`, confidence floor |
| System state | current interval, previous interval, cooldown state, operator mode |
| Uncertainty | sample size, freshness, confidence, conflicting signals |
| Action space | numeric drop interval in `50ms` steps |
| Fallback contract | `800ms` default interval |
| Audit/explanation | reason, evidence, policy result, returned value |

## Design guidance

Flaggo should not overload the word "context". Instead:

- use **runtime context** for current facts passed at decision time,
- use **telemetry evidence** for observed behavior over time,
- use **decision surface** for the application code integration point,
- use **policy constraints** for hard boundaries,
- use **goals** for human intent,
- use **system state** for control-plane memory,
- use **uncertainty** for evidence quality,
- use **action space** for possible outputs,
- use **fallback contract** for safe behavior when decisioning cannot proceed.

This vocabulary should guide future SDK, API, frontend, and storage design.
