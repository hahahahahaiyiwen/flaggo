# Tetris drop-speed scenario

## Purpose and boundary

The game delegates `tetris.dropInterval` while retaining ownership of applying
the result and emitting application telemetry. The scenario demonstrates the
v3 contract lifecycle and exact-version runtime path without request-time AI,
target resolution, durable decision records, confirmation sessions, or SDK
fallback.

| Concern | Tetris contract |
| --- | --- |
| Management identity | Name `tetris.dropInterval`; immutable versions use `contractDigest` |
| Runtime identity | Exact `{ contractName, contractDigest }` |
| Runtime attributes | `board_pressure`, `recent_placement_time_ms`, `recovery_failures`, `current_level`, `session_id` |
| Result | Number `200..1500ms`, multiple of `50ms`, default `800ms` |
| Runtime authority | One active immutable executable for the authenticated scope and exact digest |
| Learning | Auto-activation policy with placement-time objective and recovery-failure guardrail |

## Flow

```text
decision-contract.json
  -> Contract Service validation and PUT
  -> immutable accepted contract version
  -> default executable activation
  -> authored executable validation and activation
  -> returned { contractName, contractDigest }

application attributes + SDK-owned _random
  -> exact-version Decision Service request
  -> active executable evaluation
  -> RuntimeDecision
  -> game applies result
  -> application-owned telemetry
```

The Decision Service does not consult the management current pointer. Missing
attributes make dependent rules ineligible; when no rule is eligible, runtime
returns the contract default.

## Authored executable

The integration contract uses two ordered rules:

```text
board_pressure >= 0.8 or recovery_failures >= 3 -> 850ms
board_pressure < 0.8 and recovery_failures < 3  -> 750ms
no eligible rule                                -> 800ms default
```

```ts
const decision = await flaggo.decide<number>("tetris.dropInterval", {
  attributes: {
    board_pressure: boardPressure,
    recent_placement_time_ms: recentPlacementTimeMs,
    recovery_failures: recoveryFailures,
    current_level: currentLevel,
    session_id: sessionId,
  },
});

applyDropInterval(decision.result);
```

The SDK binding contains the exact digest returned by Contract Service. The
SDK constructs the complete `RuntimeInput`, adds `_random`, and reuses the
serialized input on retries.

## Required behavior

| Situation | Outcome |
| --- | --- |
| High pressure or repeated recovery failure | Rule result `850ms` |
| Lower pressure without repeated recovery failure | Rule result `750ms` |
| Attributes needed by both rules are missing | Contract default `800ms` |
| No active executable for the exact digest | Explicit `executable-not-active` failure |
| Decision Service unavailable | Explicit transport/service failure; no SDK fallback |
| Invalid persisted authority | Explicit integrity failure; never implicit repair |

The automated integration starts Contract Service and Decision Service against
one isolated SQLite database, publishes the contract, evaluates all three
result paths, and verifies that retired v1 request fields are absent.

## Related documents

- [Architecture overview](../design/architecture/OVERVIEW.md)
- [Contract Service](../design/architecture/CONTRACT_SERVICE.md)
- [Runtime client and Decision Service](../design/architecture/RUNTIME.md)
- [Tetris integration](../../examples/tetris-integration/README.md)
