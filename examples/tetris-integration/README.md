# Interactive Tetris and Flaggo integration

This example is a playable terminal Tetris application. The game owns its
board, pieces, controls, scoring, levels, and gravity scheduling. It runs
standalone with a local gravity policy and can optionally ask Flaggo for the
drop interval.

## Play standalone

From the repository root:

```powershell
npm run tetris
```

This build contains only the game engine, terminal UI, and local
`DropIntervalProvider`. It does not import the Flaggo SDK or require any
Flaggo service.

## Play with local Flaggo services

Install the npm dependencies, .NET 10 SDK, and repository-pinned Rust
toolchain, then run:

```powershell
npm run tetris:flaggo
```

The command builds and starts Contract Service, Decision Service, and OTel
Ingestion against an isolated SQLite database, then deploys the contracts
listed in `flaggo.deploy.json`. It binds the returned immutable contract digest
and starts the game. All services and the temporary database are stopped and
removed when the game exits.

The game starts immediately with local gravity while requesting its first
Flaggo interval in the background. It summarizes a trailing five-second
observation window and refreshes the decision every five seconds, with at most
one request in flight. Piece locks update that window but do not trigger
requests. Responses update the value used by future gravity ticks without
resetting an already scheduled tick.

If Flaggo is unavailable before the first successful response, the app keeps
running with local level-based gravity and displays `Local fallback`. After a
successful response, a failed refresh retains the last Flaggo interval and
displays `Flaggo cached`. This is application behavior implemented by the
terminal and optional provider; the SDK does not synthesize fallback decisions.

The application owns its OpenTelemetry logger, meter, and tracer providers and
adds the standard Flaggo OTLP processors and metric reader. All three signals
export directly to OTel Ingestion; no Collector process is required. An OTLP
success response means that the complete export request was durably enqueued.
Evidence selection and materialization continue asynchronously.

### Telemetry design

The telemetry boundary keeps game rules independent of OpenTelemetry.
`TetrisSession` publishes typed lifecycle and command transitions through
`TetrisSessionInstrumentation`; `session-telemetry.ts` maps them to OTel.
`otel.ts` remains the composition root that owns resources, providers,
exporters, explicit force-flush, and shutdown.

| Scope | Responsibility |
| --- | --- |
| `tetris.engine` | Session lifecycle, commands, pieces, lines, state, and raw evidence candidates |
| `tetris.policy` | Decision-window features, policy selection logs, and `tetris.drop_interval.select` spans |
| `@flaggo/sdk` | Canonical `flaggo.decision.received` events |

Engine logs cover game start/restart, pause/resume, piece spawn/lock, line
clear, rejected recovery, and game over. Every programmatic command creates a
`tetris.command` span; lock, line-clear, recovery-failure, and game-over facts
are span events. Policy selection logs and the SDK decision event correlate
with the active policy span.

The metric catalog separates future evidence from operational telemetry:

| Metrics | Attributes and purpose |
| --- | --- |
| `tetris.placement_time`, `tetris.recovery_failure` | Carry `tetris.session.id`; these names match the decision contract's `tetris.placement_time` and `tetris.recovery_failure` evidence bindings |
| `tetris.board.pressure`, `tetris.score`, `tetris.level` | Per-session state gauges |
| `tetris.game.started`, `tetris.game.completed`, `tetris.session.active` | Aggregate lifecycle counts |
| `tetris.command` | Aggregate command count by bounded command and outcome |
| `tetris.piece.spawned`, `tetris.piece.locked`, `tetris.lines.cleared`, `tetris.drop.distance` | Aggregate engine operations with only bounded piece or command attributes |
| `tetris.*_5s` policy metrics | Exact rolling attributes sent to the decision contract |

The engine does not log gravity ticks or export board matrices. Session IDs are
reserved for logs, spans, evidence candidates, and per-session state; aggregate
operational counters avoid session-cardinality growth. No instrumentation
scope schema URL is set until a Tetris schema is published.

## Headless session API

`TetrisSession` is the reusable application boundary beneath the terminal. It
owns one game's board, rolling decision observations, current drop-interval
selection, fallback/cached-policy behavior, pause/resume, restart, and policy
request cancellation. It does not read a TTY or create timers; callers decide
when to send gravity ticks and request policy refreshes.

```javascript
import { TetrisSession } from "./dist/standalone/session.js";

const session = new TetrisSession({
  provider,
  game: { sessionId: "programmatic-game-1" },
});

session.dispatch("move-left");
session.dispatch("rotate-clockwise");
const placement = session.dispatch("hard-drop");
const policy = await session.refreshPolicy();
const state = session.snapshot();
```

Commands are `move-left`, `move-right`, `rotate-clockwise`, `soft-drop`,
`hard-drop`, `gravity-tick`, `pause`, `resume`, and `restart`. Every transition
returns a caller-owned state snapshot with the visible board, score, lines,
level, lifecycle status, session ID, monotonic revision, current policy
selection, and decision observation. Multiple sessions can share a provider
while retaining independent board, policy, cancellation, and lifecycle state.
Default session IDs are collision-resistant; deterministic callers should
supply explicit IDs, clocks, and piece sources.

Callers may also pass one shared `TetrisSessionInstrumentation` implementation
to multiple sessions. The application-owned OTel adapter preserves session
correlation while the standalone build continues to have no OTel or Flaggo
runtime dependency.

`runTerminalTetris` is only a driver over this API: terminal keypresses become
commands, and terminal timers decide when to send gravity and policy-refresh
operations.

## Controls

| Action | Keys |
| --- | --- |
| Move left or right | Left/Right arrows or `A`/`D` |
| Rotate clockwise | Up arrow or `W` |
| Soft drop | Down arrow or `S` |
| Hard drop | Space |
| Pause or resume | `P` |
| Restart | `R` |
| Quit | `Q` or Ctrl+C |

The board is 10x20 and uses a seven-bag piece source. The engine supports all
seven tetrominoes, clockwise rotation with simple horizontal kicks, line
clearing, scoring, levels, soft drop, hard drop, pause, restart, and game over.

## Decision contract

`flaggo/contracts/tetris.dropInterval.decision-contract.json` defines
`tetris.dropInterval`. The file name must preserve the exact decision name and
end in `.decision-contract.json`; `flaggo.deploy.json` references the file.

| Condition | Interval |
| --- | --- |
| Five-second mean pressure is at least `0.75`, maximum pressure is at least `0.9`, or recovery failures are at least `3` | `850ms` |
| Lower mean and maximum pressure with fewer than `3` recovery failures | `750ms` |
| No authored rule is eligible | Contract default `800ms` |

Runtime attributes state their temporal meaning explicitly:
`board_pressure_mean_5s`, `board_pressure_max_5s`,
`placement_time_mean_ms_5s`, `recovery_failures_5s`,
`pieces_locked_5s`, `current_level`, and `session_id`.

The optional adapter uses `createDecisionClient` from `@flaggo/sdk/runtime`.
The deployment helper uses `createContractClient` from
`@flaggo/sdk/management` after Contract and Decision Services are ready. Its
`deploy` operation sends the authoritative `PUT`; it does not call the optional
remote validation endpoint first.

## Automated checks

Run deterministic engine, provider, renderer, and terminal-cleanup tests:

```powershell
npm run test:tetris-app
```

Run the real-host integration independently:

```powershell
npm run test:tetris-integration
```

The real-host harness drives two deterministic headless sessions through a
line clear and rejected recovery actions, verifies the `850` and `750` paths,
captures logs, metrics, and traces while forwarding them to the Rust OTLP
receiver, and checks resource/scope metadata, cross-signal correlation,
session isolation, future evidence candidates, and deliberately unmatched
operational telemetry. The test compares the forwarded request count and
decompressed byte total with OTel Ingestion's raw-inbox health, then restarts
the receiver on the same SQLite database and verifies those retained values
survive reopening. It also verifies SDK-owned `_random`, exact digest
provenance, direct REST parity from the captured session attributes,
retired-field absence, explicit failure after Decision Service stops, and
contract persistence across restart. Exporters are force-flushed before
assertions, so an export, receiver, or durable-inbox failure fails the run.
