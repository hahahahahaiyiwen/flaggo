# Tetris drop-speed scenario

## Purpose and boundary

Tetris is a complete application before Flaggo is introduced. The game owns
board state, player input, piece placement, scoring, levels, and application of
the current gravity interval. A `DropIntervalProvider` is the narrow boundary
through which gravity policy can vary:

```text
TetrisGame
  -> DropIntervalContext
  -> DropIntervalProvider
       -> LocalDropIntervalProvider
       -> FlaggoDropIntervalProvider
            -> Decision Service
            -> LocalDropIntervalProvider on failure
  -> selected interval
  -> terminal game loop
```

The local provider makes the game independently usable. The Flaggo provider is
an optional adapter, dynamically loaded only by the Flaggo launch path. Its
failure branch visibly selects local gravity; that branch is application
policy, not SDK fallback.

| Concern | Owner |
| --- | --- |
| Game state and deterministic mechanics | `TetrisGame` |
| Local level-based gravity | `LocalDropIntervalProvider` |
| Runtime context snapshot | `TetrisGame` |
| Exact-version decision request | `FlaggoDropIntervalProvider` and SDK |
| Active executable resolution and evaluation | Decision Service |
| Applying the selected interval | Terminal game loop |
| Degraded-mode status | Terminal game loop |

## Contract and runtime authority

| Concern | Tetris contract |
| --- | --- |
| Management identity | Name `tetris.dropInterval`; immutable versions use `contractDigest` |
| Runtime identity | Exact `{ contractName, contractDigest }` |
| Runtime attributes | `board_pressure`, `recent_placement_time_ms`, `recovery_failures`, `current_level`, `session_id` |
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

piece-lock context + SDK-owned _random
  -> exact-version Decision Service request
  -> active executable evaluation
  -> RuntimeDecision
  -> provider selects interval
  -> game loop applies interval
```

The provider evaluates at startup and after a piece locks. It does not make a
request for every gravity tick. Decision Service does not consult the
management current pointer. Missing attributes make dependent rules ineligible;
when no rule is eligible, runtime returns the contract default.

## Authored executable

The contract uses two ordered rules:

```text
board_pressure >= 0.8 or recovery_failures >= 3 -> 850ms
board_pressure < 0.8 and recovery_failures < 3  -> 750ms
no eligible rule                                -> 800ms default
```

The adapter declares the decision catalog and binds the exact accepted digest:

```ts
type TetrisDecisions = {
  "tetris.dropInterval": DecisionSpec<{
    board_pressure: number;
    current_level: number;
    recent_placement_time_ms: number;
    recovery_failures: number;
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
    board_pressure: context.boardPressure,
    current_level: context.currentLevel,
    recent_placement_time_ms: context.recentPlacementTimeMs,
    recovery_failures: context.recoveryFailures,
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
| High pressure or repeated recovery failure | Flaggo rule result `850ms` |
| Lower pressure without repeated recovery failure | Flaggo rule result `750ms` |
| Attributes needed by both rules are missing | Contract default `800ms` |
| No active executable for the exact digest | Provider shows degraded status and applies local policy |
| Decision Service unavailable | Provider shows degraded status and applies local policy |
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
