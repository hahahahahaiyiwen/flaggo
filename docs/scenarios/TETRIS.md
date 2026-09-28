# Tetris drop-speed scenario

## Purpose and boundary

Tetris is a complete application before Flaggo is introduced. The game owns
board state, player input, piece placement, scoring, levels, and application of
the current gravity interval. A `DropIntervalProvider` is the narrow boundary
through which gravity policy can vary:

```text
TetrisGame
  -> DropIntervalObservation
  -> rolling five-second DropIntervalContext
  -> periodic policy-refresh loop
  -> DropIntervalProvider
       -> LocalDropIntervalProvider
       -> FlaggoDropIntervalProvider
            -> Decision Service
            -> LocalDropIntervalProvider on failure
  -> cached selected interval

terminal game loop
  -> gravity timer
  -> cached selected interval
```

The local provider makes the game independently usable. The Flaggo provider is
an optional adapter, dynamically loaded only by the Flaggo launch path. Its
failure branch visibly selects local gravity; that branch is application
policy, not SDK fallback. Gameplay and policy refresh are concurrent regions:
network completion never blocks input or resets an already scheduled gravity
tick.

| Concern | Owner |
| --- | --- |
| Game state and deterministic mechanics | `TetrisGame` |
| Local level-based gravity | `LocalDropIntervalProvider` |
| Point-in-time game observation | `TetrisGame` |
| Five-second feature aggregation | Terminal integration |
| Exact-version decision request | `FlaggoDropIntervalProvider` and SDK |
| Active executable resolution and evaluation | Decision Service |
| Refresh cadence and latest-success cache | Terminal integration |
| Applying the cached interval | Terminal game loop |
| Degraded-mode status | Terminal game loop |

## Contract and runtime authority

| Concern | Tetris contract |
| --- | --- |
| Management identity | Name `tetris.dropInterval`; immutable versions use `contractDigest` |
| Runtime identity | Exact `{ contractName, contractDigest }` |
| Runtime attributes | `board_pressure_mean_5s`, `board_pressure_max_5s`, `placement_time_mean_ms_5s`, `recovery_failures_5s`, `pieces_locked_5s`, `current_level`, `session_id` |
| Result | Number `200..1500ms`, multiple of `50ms`, default `800ms` |
| Runtime authority | One active immutable executable for the authenticated scope and exact digest |
| Learning | Auto-activation policy with placement-time objective and recovery-failure guardrail |

```text
flaggo/contracts/tetris.dropInterval.decision-contract.json
  -> flaggo.deploy.json
  -> SDK validation and Contract Service PUT
  -> immutable accepted contract version
  -> default executable activation
  -> authored executable validation and activation
  -> returned { contractName, contractDigest }

five-second rolling context + SDK-owned _random
  -> exact-version Decision Service request
  -> active executable evaluation
  -> RuntimeDecision
  -> refresh loop caches interval
  -> a future gravity tick uses the cached interval
```

The game starts from local gravity and requests the first decision
asynchronously. While the game is running, it refreshes every five seconds with
at most one request in flight. Piece locks contribute observations but do not
trigger requests. Refresh stops while paused or after game over, and restart
creates a new observation window. Decision Service does not consult the
management current pointer. Missing attributes make dependent rules ineligible;
when no rule is eligible, runtime returns the contract default.

## Authored executable

The contract uses two ordered rules:

```text
board_pressure_mean_5s >= 0.75
  or board_pressure_max_5s >= 0.9
  or recovery_failures_5s >= 3              -> 850ms
lower pressure and fewer recovery failures  -> 750ms
no eligible rule                            -> 800ms default
```

The adapter declares the decision catalog and binds the exact accepted digest:

```ts
type TetrisDecisions = {
  "tetris.dropInterval": DecisionSpec<{
    board_pressure_mean_5s: number;
    board_pressure_max_5s: number;
    current_level: number;
    placement_time_mean_ms_5s: number;
    recovery_failures_5s: number;
    pieces_locked_5s: number;
    session_id: string;
  }, number>;
};

const client = createDecisionClient<TetrisDecisions>({
  baseUrl: decisionServiceUrl,
  bindings: {
    "tetris.dropInterval": { contractDigest },
  },
});

const response = await client.decide("tetris.dropInterval", {
  attributes: {
    board_pressure_mean_5s: context.boardPressureMean5s,
    board_pressure_max_5s: context.boardPressureMax5s,
    current_level: context.currentLevel,
    placement_time_mean_ms_5s: context.placementTimeMeanMs5s,
    recovery_failures_5s: context.recoveryFailures5s,
    pieces_locked_5s: context.piecesLocked5s,
    session_id: context.sessionId,
  },
});

applyDropInterval(response.value.result);
```

The SDK constructs complete `RuntimeInput`, adds `_random`, and reuses the
serialized input on retries.

## Required behavior

| Situation | Outcome |
| --- | --- |
| Standalone mode | Local interval from level, with no SDK or service dependency |
| Startup before the first response | Gameplay starts immediately with local gravity |
| Piece lock | Rolling observations update; no request is sent |
| Five-second refresh while a request is active | Refresh is skipped; requests never overlap |
| High pressure or repeated recovery failure | Flaggo rule result `850ms` |
| Lower pressure without repeated recovery failure | Flaggo rule result `750ms` |
| Attributes needed by both rules are missing | Contract default `800ms` |
| No active executable for the exact digest | Provider shows degraded status and applies local policy |
| Decision Service unavailable before success | Provider shows degraded status and applies local policy |
| Refresh failure after success | Last successful Flaggo interval remains cached |
| Pause or game over | Policy refresh stops |
| Invalid persisted authority | Decision Service returns an explicit integrity failure; never implicit repair |

The automated real-host integration remains separate from interactive play. It
starts Contract Service and Decision Service against one isolated SQLite
database, deploys the contract, evaluates all three result paths, and
verifies that retired v1 request fields are absent.

## Related documents

- [Architecture overview](../design/architecture/OVERVIEW.md)
- [Contract Service](../design/architecture/CONTRACT_SERVICE.md)
- [Runtime client and Decision Service](../design/architecture/RUNTIME.md)
- [Interactive Tetris example](../../examples/tetris-integration/README.md)
