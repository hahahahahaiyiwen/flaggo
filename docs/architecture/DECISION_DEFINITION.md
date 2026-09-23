# Decision definition

## Purpose

A decision definition is the versioned semantic contract for one Flaggo
decision. It declares targets, typed runtime inputs, output space, intent,
deterministic constraints, fallback, and authority workflow.

The canonical static source is a hand-authored manifest. A trusted
deployment/control-plane client submits that manifest to Contract Service.
Runtime application code references the registered decision by key and sends
only live target, context, and input data.

## Identity

```text
decision key:
  tetris.dropInterval

definition lineage:
  definitionId: def_01JQ8Y7M6X3K9P2W4R5T6V7N8A

runtime identity:
  revision: rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3
  contractDigest: sha256:contract...
```

The decision key is the stable application-facing name. Contract Service
assigns the opaque lineage ID and revision. The complete
`{ definitionId, revision, contractDigest }` tuple is the immutable runtime
identity.

## Definition-owned semantics

| Part | Meaning | Tetris example |
| --- | --- | --- |
| Key | Stable decision family. | `tetris.dropInterval` |
| Target hierarchy | Permitted resolution levels and order. | `session -> cohort -> global` |
| Runtime inputs | Typed values resolved from declared request or authorized evidence sources. | board pressure, placement time, failures, level |
| Signal references | Immutable observation/evidence identities. | early-loss rate, hard-drop rate |
| Intent | Human or metric objective for async analysis. | reduce early loss while preserving challenge |
| Action space | Output type, bounds, allowed values, step, and default. | number `200..1500`, step `50`, default `800` |
| Constraints | Deterministic rules that may narrow but never widen authority. | fixed-default `max-delta = 50` |
| Fallback | Governed value for an otherwise valid ready request. | `800ms` |
| Lifecycle | Authority source and initial candidate where applicable. | bundle-approved numeric rule |

The definition does not own telemetry history, evidence views, active state,
decision records, exposures, outcomes, or app/build provenance.

## Canonical manifest

The manifest declares static semantics without runtime values:

```yaml
format: flaggo.decision-definition-bundle/v2
application:
  id: tetris-demo
  environment: dev
source:
  repository: hahahahahaiyiwen/flaggo
  path: examples/tetris-integration/tetris-manifest.yaml
definitions:
  - key: tetris.dropInterval
    valueType: number
    targetHierarchy: [session, cohort, global]
    runtimeContextSchema:
      sessionId:
        type: string
        required: true
        target: session
      cohort:
        type: string
        required: true
        target: cohort
    inference:
      target: session
      fallbackOrder: [cohort, global]
      inputs:
        - key: tetris.boardPressure
          valueType: number
          source: { kind: request, field: boardPressure }
        - key: tetris.recentPlacementTimeMs
          valueType: number
          source: { kind: request, field: recentPlacementTimeMs }
        - key: tetris.recoveryFailures
          valueType: number
          source: { kind: request, field: recoveryFailures }
        - key: tetris.currentLevel
          valueType: number
          source: { kind: request, field: currentLevel }
    actionSpace:
      type: number
      min: 200
      max: 1500
      step: 50
      default: 800
    fallback:
      value: 800
    constraints:
      rules:
        - kind: max-delta
          value: 50
    lifecycle:
      authorityMode: bundle-approved
      initialAuthority:
        kind: numeric-rule
        controlTarget:
          type: cohort
          id: new_players
        rule:
          threshold: 0.55
          valueAtOrAbove: 850
          valueBelow: 750
          weightedInputs:
            - input: { key: tetris.boardPressure }
              minimum: 0
              maximum: 1
              weight: 0.45
            - input: { key: tetris.recentPlacementTimeMs }
              minimum: 0
              maximum: 2000
              weight: 0.25
            - input: { key: tetris.recoveryFailures }
              minimum: 0
              maximum: 5
              weight: 0.20
            - input: { key: tetris.currentLevel }
              minimum: 0
              maximum: 20
              weight: 0.10
        rationale: Initial deterministic Tetris behavior.
```

Issue #44 owns the final manifest syntax, typed input resolution, and OTel
binding shape. Issue #40 adds the bundle-approved lifecycle and target
constraint names without reintroducing inline application definitions.

## Signal and input ownership

Signal declarations are immutable manifest data with stable keys, types, units,
ranges, sources, and derived semantics.

```yaml
signals:
  - key: tetris.boardPressure
    kind: metric
    type: number
    source: app-emitted
    range: [0, 1]
  - key: tetris.earlyLossRate24h
    kind: metric
    type: number
    source: derived
    from: [tetris.sessionEnded]
    aggregation: rate(endReason == 'early_loss')
    window: 24h
```

A semantic schema change requires a new key. A derived metric's aggregation and
window are part of that immutable meaning.

Runtime inputs are a narrower role. Every input has an explicit source owned by
#44's manifest/input contract:

- request-sourced values arrive with the decision request;
- evidence-sourced values are resolved from an authorized, materialized,
  target-specific Evidence Store view;
- Decision Service receives the same typed resolved-input collection regardless
  of source; and
- Decision Service records values plus source provenance used for a successful
  server result.

The bounded numeric-rule executor never queries raw telemetry, waits for OTel
export, or consumes an `EvidenceSnapshot`. The Tetris Phase 3 rule uses only
request-sourced inputs. Exact input-source syntax and supported evidence
projections remain owned by #44.

## Intent

Intent guides future Async Analysis Pipeline work. It is not executable online
authority.

```yaml
intent:
  type: metric-objective
  primary:
    signal: tetris.earlyLossRate24h
    direction: minimize
  secondary:
    - signal: tetris.hardDropRate24h
      direction: target
      target: 0.45
```

Objective signals must be numeric. `target` requires a finite numeric target;
`minimize` and `maximize` forbid one.

## Decision constraints

Every definition carries complete `DecisionConstraints` data. Phase 3 does not
support a separately resolved constraint reference or environment policy.

Contract Service validates:

- action-space and fallback coherence;
- constraint shape and duplicate kinds;
- candidate target and output compatibility;
- required input declarations; and
- initial authority compatibility.

Decision Service evaluates the same accepted constraint projection against the
exact runtime candidate. Constraints can approve, block, or require governed
fallback. They cannot clamp, repair, or widen approved authority.

Issue #40 replaces the executable `policy`, `InlinePolicy`,
`PolicyConstraint`, and `PolicyEvaluationResult` names with `constraints`,
`DecisionConstraints`, `DecisionConstraint`, and
`ConstraintEvaluationResult`.

## Authority workflow

```text
canonical manifest + initial authority candidate
  -> Contract Service canonical validation
  -> authenticated exact-snapshot approval
  -> expected stable-head baseline
  -> State Store compare-and-swap activation
  -> ready runtime binding
```

The manifest may describe a bounded candidate. It cannot supply trusted
approval, proposal, activation, state, strategy, or decision-record identities.

Async Analysis Pipeline may later submit candidates through the same Contract
Service boundary. It cannot approve or activate them directly.

## Runtime projection and request

Decision Service receives an immutable projection containing only behavior
needed online:

- exact runtime identity;
- target hierarchy and fallback order;
- typed runtime input declarations;
- action space and fallback;
- decision constraints; and
- authority compatibility requirements.

Application code is key-based:

```ts
const result = await flaggo.decide.number("tetris.dropInterval", {
  target: { type: "session", id: sessionId },
  context: {
    sessionId,
    cohort: playerCohort
  },
  inputs: {
    "tetris.boardPressure": boardPressure,
    "tetris.recentPlacementTimeMs": recentPlacementTimeMs,
    "tetris.recoveryFailures": recoveryFailures,
    "tetris.currentLevel": game.level
  }
});

gameEngine.updateConfig({ dropInterval: result.value });
```

No inline definition, policy, signal handle, AST extraction, or management
credential belongs in the runtime call site.

## Versioning

Semantic changes create a new digest/revision, including changes to:

- action space or fallback;
- targets, context bindings, or fallback order;
- input/signal roles;
- intent or objectives;
- decision constraints; or
- authority mode, target, kind-specific candidate, rule, or rationale.

Metadata-only edits may retain semantic identity only when Contract Service can
prove runtime behavior is unchanged.

Publication is independent from application deployment. A trusted deployment,
CLI, CI/CD, GitOps, or operator client submits the manifest. Decision Service
never registers definitions from runtime traffic.

## Durable records

Contract Store retains definitions, revisions, approvals, baselines, and
readiness metadata. State Store retains activation and immutable authority.
Evidence Store retains decision, exposure, and outcome records.

These records make lifecycle and runtime behavior reconstructable without an
Audit component. Explanation is derived from the stored facts.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Evidence](EVIDENCE.md)
- [Authority](AUTHORITY.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Shared contracts](../design/shared-contracts/README.md)
- [Contract Service](../design/contract-service/README.md)
