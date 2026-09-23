# Tetris drop-speed scenario

## Purpose

The first Flaggo product slice delegates:

```text
tetris.dropInterval
```

The game adapts drop speed to the current session while remaining inside an
explicit definition, approved authority, deterministic constraints, durable
record, fallback, and attribution boundary.

## Contract

| Concern | Tetris value |
| --- | --- |
| Runtime identity | Exact `{ definitionId, revision, contractDigest }` |
| Runtime target | `session:game-456` |
| Control target | `cohort:new_players` |
| Live inputs | `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures`, `currentLevel` |
| Output | `200..1500ms`, step `50ms`, default `800ms` |
| Authority | Bundle-approved `numeric-rule` |
| Constraints | Bounds, step, fixed-default `max-delta = 50`, required inputs |
| Durable record | Decision record committed to Evidence Store before success |
| Attribution | Exposure confirmation returns `exposureId` |

## End-to-end flow

```text
trusted deployment/control-plane client submits canonical manifest
  -> Contract Service approval and readiness
  -> State Store activation

game Client SDK requests decision by key with live inputs
  -> Decision Service loads contract and state
  -> evaluates approved numeric rule
  -> evaluates decision constraints
  -> appends durable decision record
  -> returns 750ms, 850ms, or governed 800ms fallback

game applies value
  -> confirms exposure
  -> emits exposure-linked outcomes through its OTel pipeline
  -> OTel Ingestion appends observations to Evidence Store
```

## Approved numeric rule

```text
score = sum(normalizedInput * weight) / sum(weight)

score >= 0.55 -> 850ms
score <  0.55 -> 750ms
```

| Input | Range | Weight |
| --- | --- | --- |
| `tetris.boardPressure` | `0..1` | `0.45` |
| `tetris.recentPlacementTimeMs` | `0..2000` | `0.25` |
| `tetris.recoveryFailures` | `0..5` | `0.20` |
| `tetris.currentLevel` | `0..20` | `0.10` |

The executor consumes only the runtime projection, approved rule, and live
inputs. It reports `confidence: null`.

## Required behavior

| Situation | Expected outcome |
| --- | --- |
| Score at or above `0.55` | Exact approved `850ms` branch |
| Score below `0.55` | Exact approved `750ms` branch |
| No compatible authority or constraint-required fallback | Governed `800ms` with server provenance |
| Recognized Decision Service outage with configured SDK fallback | Client `800ms`, no server decision or exposure identity |
| Unknown, conflicting, retired, or non-ready identity | Typed fallback-ineligible error |
| Invalid persisted authority | Readiness failure; never repaired |

Both rule branches satisfy fixed-default delta:

```text
abs(750 - 800) = 50
abs(850 - 800) = 50
```

## Reconstructability and attribution

Every successful server result is reconstructable from its definition, targets,
inputs, state/activation lineage, selected branch, applied constraints,
fallback provenance, result, and timestamp.

Exposure exists only after application confirmation. Outcomes link to the
confirmed `exposureId`; ordinary gameplay telemetry remains raw and unlinked.

## Phase 3 exit evidence

The final integration must prove:

1. Contract Service registers, approves, activates, and returns readiness.
2. State Store replay and stale-head protection remain exact.
3. Decision Service returns exact approved branches under deterministic
   constraints without a standalone Policy service.
4. Decision and exposure records are durable and reconstructable without a
   standalone Audit service.
5. The application uses its own OTel pipeline and Flaggo OTel Ingestion.
6. The integrated architecture uses the canonical service/store terminology.

## Related documents

- [Architecture overview](../architecture/OVERVIEW.md)
- [Authority](../architecture/AUTHORITY.md)
- [Runtime execution](../architecture/RUNTIME_EXECUTION.md)
- [Tetris integration design](../design/tetris-integration/README.md)
