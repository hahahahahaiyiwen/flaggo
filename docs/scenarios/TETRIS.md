# Tetris drop-speed scenario

## Purpose and current boundary

The game delegates `tetris.dropInterval` while retaining execution and
telemetry ownership. Flaggo returns a deterministic value inside an explicit
definition, approved authority, constraints, durable recording, fallback and
attribution boundary.

The current manifest-first integration uses existing approved numeric-rule
state provisioned by trusted local bootstrap. #49 aligns executable server
boundaries, #40 adds initial-authority/activation-ready publication, and #41
verifies the final bundle-approved path. No telemetry learning, proposal
generation, experiments, rollout or request-time AI is claimed.

| Concern | Tetris contract |
| --- | --- |
| Runtime identity | Exact `{ definitionId, revision, contractDigest }` |
| Runtime target | `session:game-456` |
| Control target | `cohort:new_players` |
| Request inputs | `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures`, `currentLevel` |
| Result | Number `200..1500ms`, step `50ms`, default `800ms` |
| Authority | Approved numeric rule; current local fixture is not a runtime authoring surface |
| Constraints | Bounds, step, fixed-default `max-delta = 50`, required inputs and explicit runtime guards |
| Recording | Durable decision record before success |
| Attribution | Explicit confirmation supplies `exposureId` for outcomes |

## Current flow

```text
one authored JSON manifest
  -> trusted Contract Service publication
  -> authenticated exact-snapshot approval -> approved-definition receipt
  -> trusted local bootstrap of receipt-bound State Store authority
  -> generated catalog + receipt initialize the runtime client

key + live inputs -> Decision Service
  -> compatible contract/state + resolved inputs
  -> approved numeric rule -> deterministic constraints
  -> durable record -> 750ms / 850ms / governed 800ms fallback

game applies result -> explicit confirmation
  -> ordinary application OTel telemetry with confirmed attributes
```

OTel Ingestion can validate declared outcome bindings against completed
confirmation and materialize attributed evidence. The separate
[Collector example](../../examples/otel-evidence/README.md) demonstrates that
bound outcome path; the current Tetris harness emits and inspects correlated
native logs without claiming a general Outcome store.

## Approved numeric rule

```text
score = sum(normalizedInput * weight) / sum(weight)
score >= 0.55 -> 850ms
score <  0.55 -> 750ms
```

| Input | Range | Weight |
| --- | --- | --- |
| `boardPressure` | `0..1` | `0.45` |
| `recentPlacementTimeMs` | `0..2000` | `0.25` |
| `recoveryFailures` | `0..5` | `0.20` |
| `currentLevel` | `0..20` | `0.10` |

The executor takes only definition, approved rule and resolved primitives.
These are live request operands, not telemetry handles, so Collector failure
does not affect their resolution. Numeric confidence is null.

```ts
const decision = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId, userId, cohort, deviceType },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});
```

The client has a generated catalog and approved receipt. Type/range/meaning,
targeting and constraints are not repeated in application call sites.

## Required behavior

| Situation | Outcome |
| --- | --- |
| Score at/above `0.55` | Exact approved `850ms` |
| Score below `0.55` | Exact approved `750ms` |
| No compatible authority or constraint-required fallback | Recorded server `800ms` |
| Recognized outage with configured SDK fallback | Client `800ms`, no server record/exposure identity |
| Invalid/unknown/conflicting/retired/non-ready identity | Explicit fallback-ineligible error |
| Invalid persisted authority | Readiness failure, never repair |

Both branches independently satisfy `abs(value - 800) <= 50`; a `750 -> 850`
sequence is valid. The existing last-change cooldown fixture proves server
fallback, not previous-result stabilization or hysteresis.

## Reconstructability and Phase 3 exit

Current records retain exact contract, targets, caller/resolved inputs,
strategy, constraint/fallback facts, value and timestamp. Exposure exists only
after application confirmation and cannot replace the input vector.
Ordinary telemetry stays unlinked when it is not caused by confirmed use.

The final #49/#40/#41 path additionally verifies activation-converged receipts,
stable-head CAS/replay, complete state/activation lineage in Evidence Store,
and server-side outcome recording under the canonical logical boundaries.
It uses no standalone Policy, Audit, Reasoning or Operator Console service.

Future async proposals still require Contract Service approval and State Store
activation. Learned strategies, experiments, rollouts and operator workflows
need their own accepted contracts.

## Related documents

- [Architecture overview](../architecture/OVERVIEW.md)
- [Authority](../architecture/AUTHORITY.md)
- [Runtime execution](../architecture/RUNTIME_EXECUTION.md)
- [Tetris integration](../design/tetris-integration/README.md)
