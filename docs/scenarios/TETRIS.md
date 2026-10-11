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
            -> fixed fallback or retained applied selection on failure
  -> cached selected interval

terminal game loop
  -> gravity timer
  -> cached selected interval
```

The local provider makes the standalone game independently usable. The Flaggo
provider is an optional adapter using the same adaptive contract and experience
tracker as the browser playground. Before its first successful selection it
retains fixed `800ms` gravity; later failures retain the applied interval and
exposure. Those branches are application policy, not SDK fallback. Gameplay and policy refresh are concurrent regions:
network completion never blocks input or resets an already scheduled gravity
tick.

| Concern | Owner |
| --- | --- |
| Game state and deterministic mechanics | `TetrisGame` |
| Local level-based gravity | `LocalDropIntervalProvider` |
| Point-in-time game observation | `TetrisGame` |
| Five-second feature and outcome aggregation | Application-owned experience tracker |
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
| Runtime attributes | Twelve scalar fields: pressure mean/max, hard-drop ratio, placement time, locked pieces, active duration, balance, lines, game overs, current interval, session ID, prospective exposure ID |
| Result | Number `200..1500ms`, multiple of `50ms`, default `800ms` |
| Runtime authority | One active immutable executable for the exact digest |
| Learning | Auto-activation with active-time-weighted pressure-band balance and throughput/game-over guardrails |

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

The Flaggo game starts from fixed `800ms` gravity and requests the first decision
asynchronously. While the game is running, it refreshes every five seconds with
at most one request in flight. Piece locks contribute observations but do not
trigger requests. Refresh stops while paused or after game over, and restart
creates a fresh session identity. A mid-piece selection applies only after
the piece locks. Decision Service does not consult the
management current pointer. Missing attributes make dependent rules ineligible;
when no rule is eligible, runtime returns the contract default.

## Authored executable

The benchmark contract uses three ordered rules:

```text
no active duration or no locked pieces -> retain current interval
SDK-owned _random < 0.5                 -> 200ms
otherwise                              -> 1000ms
```

The live playground uses five equally weighted authored arms:
`200/400/600/800/1000ms`. These are initial exploration, not runtime authority
selection by recency or a live Candidate whitelist.

The SDK constructs complete `RuntimeInput`, adds `_random`, and reuses the
serialized input on retries. Its `runtimeConfig` contains deployment authority,
service endpoints, and the exact contract binding. The shared
[`flaggo-provider.ts`](../../examples/tetris-integration/src/flaggo-provider.ts)
is the current adapter example.

## Required behavior

| Situation | Outcome |
| --- | --- |
| Standalone mode | Local interval from level, with no SDK or service dependency |
| Startup before the first response | Gameplay starts immediately with fixed `800ms` gravity |
| Piece lock | Rolling observations update; no request is sent |
| Five-second refresh while a request is active | Refresh is skipped; requests never overlap |
| No scored observations | Retain the current applied interval |
| Authored exploration with observations | Select the arm using SDK-owned `_random` |
| Attributes needed by both rules are missing | Contract default `800ms` |
| No active executable for the exact digest | Provider shows degraded status and retains fixed fallback or prior applied selection |
| Decision Service unavailable before success | Provider shows degraded status and retains fixed `800ms` gravity |
| Refresh failure after success | Last successful Flaggo interval remains cached |
| Pause or game over | Policy refresh stops |
| Invalid persisted authority | Decision Service returns an explicit integrity failure; never implicit repair |

The automated real-host integration starts Contract Service, Decision Service,
OTel Ingestion, and Evidence Materializer against one isolated database.
It checks runtime parity, OTLP transport, exact exposure/outcome correlation,
materialized evidence, and restart persistence.

The credential-free browser benchmark additionally runs Async Analysis through
an example-owned deterministic provider and a standard local Collector/LGTM
backend. It learns from nine training games, admits and auto-activates a
Candidate, then compares the active executable on three matched held-out games.
The interactive live mode uses Copilot and does not guarantee a Candidate or
human-experience improvement. Commands, guarantees, and deferred concerns are
defined in the [playground guide](../../examples/tetris-playground/README.md).

## Related documents

- [Architecture overview](../design/architecture/OVERVIEW.md)
- [Contract Service](../design/architecture/CONTRACT_SERVICE.md)
- [Runtime client and Decision Service](../design/architecture/RUNTIME.md)
- [Interactive Tetris example](../../examples/tetris-integration/README.md)
