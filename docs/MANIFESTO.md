# Flaggo Manifesto: AI-Native Runtime Decisioning

## The premise

Software is increasingly written with AI, reviewed with AI, tested with AI, and shipped through automated CI/CD systems. But once that software is running, most of its live behavior is still governed by static control flow: `if`, `else`, hard-coded thresholds, configuration values, and rule tables chosen ahead of time.

AI has entered the software production process, but it has not yet become a first-class runtime decision primitive.

Flaggo starts from a simple question:

> What if software could delegate selected runtime decisions to an AI-native decisioning layer, the same way it delegates storage to databases, delivery to queues, search to indexes, and configuration to feature flag systems?

## The problem

Modern applications contain countless decisions that are not truly static:

- Should this user enter this experience?
- Which behavior is safest for this request?
- What value should this runtime parameter take right now?
- Should this workflow continue, pause, escalate, or degrade?
- Which model, prompt, route, policy, offer, threshold, or operational action is appropriate for this context?

Today, these decisions are usually encoded through a mixture of:

- static application branches,
- feature flags,
- remote configuration,
- experimentation platforms,
- policy engines,
- dashboards and alerts,
- manual analysis,
- human operational judgment.

Each tool solves part of the problem, but the decision loop remains fragmented. Code owns execution. Feature flags own remote switches. Analytics owns evidence. Policy systems own constraints. Humans often own the final judgment. The result is latency, inconsistency, duplicated logic, and a growing gap between what the system observes and how quickly it can safely adapt.

## Beyond feature flags

Feature flags proved that application behavior does not need to be fully hard-coded. A running program can ask a remote system for a value and change behavior without redeploying.

Modern feature-flag and experimentation platforms already do much more than simple booleans: targeting, segmentation, experiments, progressive rollout, approvals, and automated rollout are common. Flaggo should not define itself by pretending those systems are only switches.

The sharper distinction is the closed loop. Feature-flag systems are strongest at delivering configured values and managing exposure. Flaggo should focus on connecting declared signals, outcomes, objectives, uncertainty, policy, and governed state so the system can learn from evidence and propose bounded changes.

That closed loop can support several parallel workflows:

- adaptive optimization improves behavior from accumulated evidence,
- experimentation creates controlled variation to generate comparative evidence,
- progressive rollout safely delivers a change that has already been selected.

These workflows share definitions, evidence, governance, attribution, and runtime delivery, but they serve different purposes. Experimentation is not another name for optimization, and rollout is not another name for experimentation.

The motivating question is not only:

> Is flag X enabled? What configured value should I deliver?

AI-native runtime decisioning asks a broader question:

> Given context, evidence, outcomes, objectives, policy, governed state, and uncertainty, what safe behavior should happen now?

The distinction matters. A flag system externalizes and governs a variable. A decisioning system adds explicit closed-loop evidence learning, constrained strategy generation, governed state compatibility, and uncertainty-aware explanations around that variable.

Flaggo is motivated by the belief that the next abstraction is not a smarter flag. It is a policy-first decisioning control plane with a runtime decision provider.

## The vision

Flaggo imagines a future where programs can call a runtime decision provider as naturally as they call a database or service API, while a governed control plane manages how those decisions are defined, optimized, experimented with, rolled out, and changed.

In that future, selected runtime choices are not buried in static branches or scattered across dashboards, scripts, and manual processes. They are explicit, observable, policy-controlled decision interfaces.

The application still owns execution. The product and platform teams still define intent, boundaries, and accountability. But the decisioning layer helps evaluate context, evidence, goals, and constraints at runtime.

The goal is not to replace code. The goal is to give code a native way to ask for policy-gated runtime decision results when static logic is too rigid, stale, or context-blind.

## What AI-native runtime decisioning should mean

AI-native runtime decisioning is not unbounded autonomy. It is not an LLM freely choosing arbitrary behavior inside production systems. It is not replacing engineering judgment with opaque model output.

It should mean:

- decisions are explicit rather than hidden in scattered code paths,
- runtime context and telemetry can influence behavior,
- policies and constraints are first-class,
- uncertainty is acknowledged instead of ignored,
- decisions are explainable and auditable,
- human intent remains encoded in goals and boundaries,
- fallback behavior exists when confidence, evidence, or safety is insufficient.

AI should primarily participate in asynchronous analysis: interpreting evidence, detecting drift, comparing strategies, generating `DecisionProposal` objects, and explaining tradeoffs. Online execution should normally be deterministic, bounded, compatible with approved `GovernedDecisionState`, and policy-gated.

This is the standard Flaggo should hold itself to: not "AI makes choices" but "software gains a policy-gated decision interface that can use AI where AI is appropriate."

## Why now

Several trends are converging:

- AI is becoming a normal part of software creation.
- Runtime systems already depend on remote configuration, feature flags, policy engines, and experimentation platforms.
- Observability has made production behavior measurable in near real time.
- Teams increasingly need adaptive behavior across users, segments, workloads, models, costs, safety constraints, and business goals.
- Static branches and manually tuned thresholds cannot keep up with the complexity and speed of modern software environments.

The missing layer is a disciplined runtime decision abstraction that connects intent, evidence, policy, and action.

## The aspiration

Flaggo exists to explore that missing layer.

It should help software move from static branching toward governed runtime judgment; from isolated flags toward contextual decisions; from manual tuning toward closed-loop adaptation; from opaque automation toward auditable autonomy.

The long-term ambition is for AI-native runtime decisioning to become a normal software primitive:

```text
definition + evidence + outcomes + objectives
  -> optimization, experiment, or rollout lifecycle
  -> proposal -> governance -> governed state

definition + runtime context + compatible governed state + policy
  -> fixed resolution, strategy evaluation, variant assignment,
     rollout routing, override, or fallback
  -> runtime decision result
```

Not every branch should become a decision call. Not every decision needs AI. Not every system should adapt automatically.

But for the decisions that are contextual, evidence-driven, high-change, and policy-constrained, software deserves a better primitive than hard-coded logic plus manual operations.

Flaggo is the attempt to define that primitive.
