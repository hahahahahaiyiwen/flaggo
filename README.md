# Flaggo

Flaggo is an exploration of **AI-native runtime decisioning**: a governed way for running software to ask for contextual decisions when static code, feature flags, and manual tuning are too rigid.

The core question is:

> Given a decision surface, scope, runtime context, telemetry evidence, goals, policy constraints, system state, uncertainty, action space, and fallback contract, what should happen now?

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

- domain telemetry events such as `hard_drop_pressed`, `piece_placed`, and `game_ended`,
- decision evidence such as hard-drop rate and early-loss rate,
- a decision surface: `tetris.dropInterval`,
- an action space: `200ms` to `1500ms`,
- goals: keep gameplay challenging but playable,
- policies: max delta, cooldown, sample-size minimum, confidence floor,
- fallback: `800ms`.

At runtime:

```text
game emits telemetry
  -> game asks Flaggo for tetris.dropInterval
  -> Flaggo evaluates evidence, scope, goals, policy, state, and uncertainty
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
- [Decision Surface](docs/DECISION_SURFACE.md)
- [Decision Scope](docs/DECISION_SCOPE.md)
- [Decision Factors](docs/DECISION_FACTORS.md)
- [Component Design Index](docs/design/README.md)

## Repository layout

```text
docs/          Product, concept, and component design documents
repos/         Related implementation repositories and experiments
```

