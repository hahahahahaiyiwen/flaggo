# Tetris drop-speed scenario

## Purpose

The first Flaggo product slice delegates one concrete runtime variable:

```text
tetris.dropInterval
```

The game should adapt drop speed to the current session while remaining inside
an explicit contract, policy, approval, audit, fallback, and attribution
boundary.

This scenario owns the measurable product behavior. Architecture and SDK/API
details live in their canonical documents.

## Current boundary

The Phase 3 scenario uses a bundle-authored deterministic numeric rule. It does
not claim telemetry learning, evidence-backed proposal generation,
experimentation, rollout, or request-time AI.

| Concern | Tetris contract |
| --- | --- |
| Decision key | `tetris.dropInterval` |
| Runtime identity | Exact `{ definitionId, revision, contractDigest }` accepted during registration |
| Runtime target | `session:game-456` |
| Control target | `cohort:new_players` |
| Live inputs | `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures`, `currentLevel` |
| Output contract | Number from `200ms` through `1500ms`, step `50ms` |
| Fixed default | `800ms` |
| Authority | Bundle-approved `numeric-rule` |
| Policy | Bounds, step, fixed-default `max-delta = 50`, and declared runtime constraints |
| Audit | Durable decision audit committed before success |
| Attribution | Exposure confirmation returns `exposureId`; outcomes link to it |

## End-to-end flow

```text
developer declares definition + initial numeric rule
  -> trusted control-plane client applies bundle
  -> authenticated actor approves exact snapshot
  -> service activates governed state
  -> registration becomes ready

game sends exact identity + session context + live inputs
  -> Flaggo resolves cohort authority
  -> evaluates the approved rule
  -> applies runtime policy
  -> durably records audit
  -> returns 750ms, 850ms, or governed 800ms fallback

game applies returned value
  -> confirms exposure when required
  -> receives exposureId
  -> emits outcome telemetry linked to exposureId
```

The browser never owns management credentials. Local development may use a
trusted bootstrap host or an explicitly insecure local-only mode.

## Approved numeric rule

The rule normalizes each live input to its declared range, computes the
normalized weighted average, and selects one exact approved branch:

```text
score = sum(normalizedInput * weight) / sum(weight)

score >= 0.55 -> 850ms
score <  0.55 -> 750ms
```

The accepted weights are:

| Input | Range | Weight |
| --- | --- | --- |
| `tetris.boardPressure` | `0..1` | `0.45` |
| `tetris.recentPlacementTimeMs` | `0..2000` | `0.25` |
| `tetris.recoveryFailures` | `0..5` | `0.20` |
| `tetris.currentLevel` | `0..20` | `0.10` |

The runtime executor consumes only the runtime definition projection, approved
numeric rule, and live signal inputs. It does not consume an
`EvidenceSnapshot`, and the bundle-authored result reports `confidence: null`.

## Required behavior

| Situation | Expected outcome |
| --- | --- |
| High board pressure and slow placement produce a score at or above `0.55`. | Return the exact approved `850ms` branch. |
| Recovery produces a score below `0.55`. | Return the exact approved `750ms` branch. |
| No permitted target has compatible authority, or runtime policy requires fallback. | Return audited server fallback `800ms` with explicit fallback provenance. |
| Ready data plane is unavailable and SDK availability fallback is explicitly enabled. | Return client fallback `800ms` without server decision, policy, audit, or exposure identity. |
| Definition identity is missing, unknown, conflicting, retired, or non-ready. | Return a typed fallback-ineligible error, not a value. |
| Persisted authority is invalid or incoherent. | Fail validation/readiness; never clamp, align, or repair the branch. |

`max-delta` uses the fixed contract default:

```text
abs(750 - 800) = 50
abs(850 - 800) = 50
```

Both branches are valid independently. A `750ms` result followed by an `850ms`
result is therefore valid under the current policy. Cooldown, hysteresis, and
previous-result stabilization are future work.

## Audit and attribution

Every successful server decision is reconstructable from:

- the exact definition identity;
- runtime and control targets;
- live input values;
- governed state, strategy, activation, and approval lineage;
- selected branch or fallback;
- policy result;
- decision and audit IDs;
- timestamp and application/build provenance.

The game confirms exposure only after applying the returned interval. The
server returns `exposureId`, and decision-caused outcomes use that identity.
Ordinary gameplay telemetry that is not caused by an applied decision remains
raw and unlinked.

## Acceptance behavior

The complete scenario proves that:

1. A developer can declare a bounded decision and initial authority candidate.
2. An authenticated actor approves the exact bundle snapshot.
3. Activation is replayable and cannot overwrite a changed stable authority
   head.
4. Registration is not ready until required authority is active.
5. Runtime returns exact `750ms` or `850ms` rule branches from live inputs.
6. Policy evaluates both branches against fixed default `800ms`.
7. A ready service durably audits before returning success.
8. Server and SDK fallback provenance remain distinct.
9. Exposure exists only after application confirmation.
10. Outcomes link to `exposureId`, not merely to a returned `decisionId`.

## Deferred behavior

Phase 4 may add an evidence-backed proposal producer and independent governance
through the shared activation boundary. Detailed proposal, experiment, rollout,
operator, rollback, and learned-strategy behavior is outside this scenario and
owned by later accepted designs.

## Related documents

- [Documentation home](../README.md)
- [Architecture overview](../architecture/OVERVIEW.md)
- [Decision definition](../architecture/DECISION_DEFINITION.md)
- [Authority](../architecture/AUTHORITY.md)
- [Runtime execution](../architecture/RUNTIME_EXECUTION.md)
- [Project roadmap](https://github.com/users/hahahahahaiyiwen/projects/3)
- [Tetris integration design](../design/tetris-integration/README.md)
