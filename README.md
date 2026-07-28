# Flaggo

Flaggo is an exploration of **AI-native runtime decisioning**: a governed way for running software to ask for contextual decisions when static code, feature flags, and manual tuning are too rigid.

The core question is:

> Given a decision definition, decision evidence, and governed state when available, what should happen now?

## Why this exists

AI is increasingly used to write, review, test, and ship code. But once code is running, most behavior is still governed by static `if/else` branches, fixed thresholds, configuration values, and manually operated feature flags.

Flaggo starts from the belief that AI should not only participate in software creation. It should also become a governed runtime decision primitive where appropriate.

## Beyond feature flags

Feature flags externalize values:

```text
Is flag X enabled?
What is config value Y?
```

Flaggo aims to externalize governed runtime judgment:

```text
Given current context, evidence, goals, policies, and uncertainty, what safe behavior should this system choose?
```

The application still owns execution. Flaggo owns the decisioning control plane around selected runtime choices.

## Hero scenario: Tetris drop speed

The first product experience uses a Tetris game:

> The game should adapt `dropInterval` so each player gets a challenging but playable experience.

Instead of hard-coding one global speed or manually tuning a feature flag, the developer declares:

- a decision key: `tetris.dropInterval`,
- a decision definition with output contract, target hierarchy, intent, safety, and fallback,
- decision evidence such as hard-drop rate, placement time, and early-loss rate,
- async intelligence that can learn a governed strategy,
- online inference that returns one safe value for the current game session.

At runtime:

```text
game emits telemetry
  -> game asks Flaggo for tetris.dropInterval
  -> Flaggo executes an approved adaptive strategy against live game context
  -> Flaggo applies governance and fallback rules
  -> Flaggo returns a governed value or fallback
  -> game applies the value
```

## Design principles

- **Explicit decisions** over hidden scattered logic.
- **Runtime context and telemetry evidence** over static assumptions.
- **Policy-first governance** over unbounded autonomy.
- **Uncertainty-aware decisions** over false precision.
- **Auditable explanations** over opaque automation.
- **Human intent encoded as goals and boundaries**.
- **Fallback behavior** when confidence, evidence, or safety is insufficient.

## Current documentation

- [Manifesto](docs/MANIFESTO.md)
- [Hero Scenario](docs/HERO_SCENARIO.md)
- [High-Level Design](docs/DESIGN.md)
- [Mental Model](docs/MENTAL_MODEL.md)
- [Decision Definition](docs/DECISION_DEFINITION.md)
- [Decision Evidence](docs/DECISION_EVIDENCE.md)
- [Decision Intelligence](docs/DECISION_INTELLIGENCE.md)
- [MVP Implementation Guide](docs/IMPLEMENTATION_GUIDE.md)
- [Component Design Index](docs/design/README.md)

## Repository layout

```text
docs/          Product, concept, and component design documents
repos/         Related implementation repositories and experiments
```
