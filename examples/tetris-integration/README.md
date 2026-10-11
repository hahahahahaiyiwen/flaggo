# Interactive Tetris and Flaggo integration

This example is a playable terminal Tetris application. The game owns its
board, pieces, controls, scoring, levels, and gravity scheduling. It runs
standalone with a local level-based gravity policy. Its Flaggo mode uses the
same adaptive decision contract, experience tracker, SDK adapter, and fixed
autonomous-player protocol as the browser playground.

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
Ingestion against an isolated SQLite database, then reads the authority-free
contract definitions listed in `flaggo.deploy.json`. Deployment injects the
manifest authority into each complete contract before the Contract Service
computes its immutable digest. After deployment, the command starts Evidence
Materializer with the deployed catalog, binds the returned authority-bound
digest into a generated `flaggo.runtime-config/v1` document, and gives that one
configuration to the decision client and all three telemetry providers before
starting the game. The manifest authority is
`local / tetris / integration`. All services and the temporary database are
stopped and removed when the game exits.

The game starts immediately with fixed `800ms` gravity while requesting its first
Flaggo interval in the background. It summarizes a trailing five-second
observation window and refreshes the decision every five seconds, with at most
one request in flight. Piece locks update that window but do not trigger
requests. Mid-piece responses are queued and apply only after the piece locks,
without resetting an already scheduled tick. Pause, restart, game over, and
close discard pending selections. Levels and line clears do not directly
accelerate Flaggo-mode gravity.

If Flaggo is unavailable before the first successful response, the app keeps
running with fixed `800ms` gravity and displays `Local fallback`. After a
successful response, a failed refresh retains the applied Flaggo interval and
exposure identity and displays `Flaggo cached`. This is application behavior implemented by the
session and optional provider; the SDK does not synthesize fallback decisions.
This interactive command does not start Async Analysis. Use the opt-in
analysis integration or the playground for the full learning/activation loop.

The application owns its OpenTelemetry logger, meter, and tracer providers and
adds the standard Flaggo OTLP processors and metric reader. All three signals
export directly to OTel Ingestion; no Collector process is required. An OTLP
success response means that the complete export request was durably enqueued.
Evidence selection and materialization continue asynchronously.

The Tetris metric reader uses the standard OTLP delta preference and retains
its five-second export cadence. Completed experience windows are recorded
once; later collections do not re-emit their four unchanged gauge points.
All window values and correlation attributes are retained without sampling.
The optional in-memory capture reader uses delta collection as well.
Counters and histograms also use delta exports; up/down counters remain
cumulative according to the standard exporter preference.

Transport remains best-effort with the existing bounded OTLP retries.
Force-flush and shutdown collect pending recordings. A terminal export failure
is reported through the existing OTel error handler, but the failed collection
is not replayed by subsequent collections; there is no durable client outbox.

`createFlaggoResource` attaches `flaggo.tenant`, `flaggo.application`, and
`flaggo.environment` from the generated runtime configuration to the one
Resource shared by every provider.

### Telemetry design

The telemetry boundary keeps game rules independent of OpenTelemetry.
`TetrisSession` publishes typed lifecycle and command transitions through
`TetrisSessionInstrumentation`; `session-telemetry.ts` maps them to OTel.
`otel.ts` remains the composition root that owns resources, providers,
exporters, explicit force-flush, and shutdown.

| Scope | Responsibility |
| --- | --- |
| `tetris.engine` | Session lifecycle, commands, pieces, lines, and operational state |
| `tetris.policy` | Exposure-correlated policy selection logs and `tetris.drop_interval.select` spans |
| `tetris.experience` | Completed, application-owned outcome windows |
| `@flaggo/sdk` | Canonical `flaggo.decision.received` events |

Engine logs cover game start/restart, pause/resume, piece spawn/lock, line
clear, rejected recovery, and game over. Every programmatic command creates a
`tetris.command` span; lock, line-clear, recovery-failure, and game-over facts
are span events. Policy selection logs and the SDK decision event correlate
with the active policy span.
Application-owned logs and spans do not carry a contract name or digest. The
SDK-owned `flaggo.decision.received` event is the exact decision-provenance
record; application evidence candidates are reusable domain observations.

The metric catalog separates future evidence from operational telemetry:

| Metrics | Attributes and purpose |
| --- | --- |
| `tetris.experience.challenge_balance`, `tetris.experience.lines_cleared`, `tetris.experience.game_over_count`, `tetris.experience.active_play_ms` | Four contract-selected gauges correlated by session and exposure, with immutable window IDs and game-clock bounds |
| `tetris.placement_time`, `tetris.recovery_failure` | Per-session engine diagnostics; no longer contract-selected learning evidence |
| `tetris.board.pressure`, `tetris.score`, `tetris.level` | Per-session state gauges |
| `tetris.game.started`, `tetris.game.completed`, `tetris.session.active` | Aggregate lifecycle counts |
| `tetris.command` | Aggregate command count by bounded command and outcome |
| `tetris.piece.spawned`, `tetris.piece.locked`, `tetris.lines.cleared`, `tetris.drop.distance` | Aggregate engine operations with only bounded piece or command attributes |

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
selection, and decision observation. Multiple sessions retain independent
board, policy, cancellation, and lifecycle state. Stateless providers can be
shared; the adaptive Flaggo provider requires a separate experience tracker,
state accessor, and allocation stream for each session.
Default session IDs are collision-resistant; deterministic callers should
supply explicit IDs, clocks, and piece sources.

Callers may also pass one shared `TetrisSessionInstrumentation` implementation
to multiple sessions. The application-owned OTel adapter preserves session
correlation while the standalone build continues to have no OTel or Flaggo
runtime dependency.

`runTerminalTetris` is only a driver over this API: terminal keypresses become
commands, and terminal timers decide when to send gravity and policy-refresh
operations.

### Autonomous greedy sessions

`runAutonomousTetrisSession` composes the same engine/session with an
independently paced current-piece greedy player. It runs without a terminal,
browser, Flaggo service, or real-time sleep:

```javascript
import { runAutonomousTetrisSession } from "./dist/standalone/autonomous-session.js";

const result = await runAutonomousTetrisSession({
  sessionId: "autonomous-game-1",
  durationMs: 120_000,
  reactionDelayMs: 3_000,
  commandIntervalMs: 200,
  dropIntervalMs: 1_000,
});

console.log(result.state.lines, result.elapsedMs, result.piecesLocked);
```

The runner advances a private virtual clock through actual gravity ticks and
paced commands in chronological order. Gravity runs first when timestamps
tie; locking starts a new piece's gravity deadline. `dropIntervalMs` is a
fixed, level-independent local interval (default `800ms`). An optional
`provider` replaces it through normal policy requests at startup and at a
piece boundary after at least five seconds. The response does not change
player reaction or command timing.

Supply `pieceSource` for repeatable scenarios, `signal` for cancellation,
`instrumentation` for ordinary session telemetry, and `onState(state, time,
transition)` for application-owned observation. The result contains the final
pre-close snapshot, elapsed game time, locked-piece count, and hard-drop
count. Every exit closes the session, including errors and cancellation.
Callers can run independent instances concurrently; use separate piece-source
and provider state when comparing policies on matched scenarios.

`runRealtimeAutonomousTetrisSession` in
`dist/flaggo/realtime-autonomous-session.js` (also in the standalone build)
accepts the same bounded episode options/result and drives native timers using
monotonic elapsed time. Gravity and fixed player commands continue while a
policy request is pending; there is at most one request in flight and
mid-piece responses wait for a lock. Scheduled gravity wins ties. Cancellation,
normal completion, and failure close the session. The runner owns one episode,
not continuous repetition; the playground owns its three live slots.

For lower-level drivers, `session.planningState()` exposes an immutable
settled-board/piece-pose snapshot. `enumerateTetrisPlacements` uses the game's
own collision, horizontal-kick rotation, and line-clear helpers.
`chooseGreedyPlacement` scores legal placements as
`3 * lines - 0.5 * aggregateHeight - 3 * holes - 0.25 * bumpiness`;
ties prefer fewer commands, then stable enumeration order.
`GreedyTetrisPlayer` selects the next legal command from the current state,
without preview, hold, or direct board mutation.

### Adaptive Flaggo composition

The shared `experience.ts`, `flaggo-provider.ts`, and `adaptive-protocol.ts`
modules live in this example. The playground imports them rather than owning
a second adapter or tracker. These are application policies, not greedy-player
responsibilities. `runTerminalTetris` also exposes `onState` and session options
so its Flaggo composition observes the same lifecycle and piece-boundary rules.

To compose an autonomous adaptive session after deploying the contract:

```javascript
import { runAutonomousTetrisSession } from "./dist/flaggo/autonomous-session.js";
import { adaptiveDemoProtocol, scenarioPieceSource, seededRandom } from "./dist/flaggo/adaptive-protocol.js";
import { createExperienceRecorder, TetrisExperience } from "./dist/flaggo/experience.js";
import { createFlaggoDropIntervalProvider } from "./dist/flaggo/flaggo-provider.js";

const seed = 11;
let state;
let clock = 0;
const experience = new TetrisExperience(
  { dataset: "training", origin: "greedy", clock: "virtual" },
  createExperienceRecorder(telemetry.getMeter("tetris.experience")));
const provider = createFlaggoDropIntervalProvider({
  runtimeConfig,
  experience,
  state: () => {
    if (state === undefined) throw new Error("Observe the session before deciding.");
    return state;
  },
  now: () => clock,
  random: seededRandom(seed ^ 0x5F3759DF),
  telemetry: telemetry.policyInstrumentation,
});
const result = await runAutonomousTetrisSession({
  sessionId: `training-${seed}`,
  durationMs: adaptiveDemoProtocol.durationMs,
  reactionDelayMs: adaptiveDemoProtocol.reactionDelayMs,
  commandIntervalMs: adaptiveDemoProtocol.commandIntervalMs,
  pieceSource: scenarioPieceSource(seed),
  instrumentation: telemetry.sessionInstrumentation,
  provider,
  onState(value, now, transition) {
    state = value;
    clock = now;
    experience.observe(value, now, transition);
  },
});
experience.completeEpisode();
console.log(result.state.lines, experience.totals());
```

`runtimeConfig` is the generated deployment configuration and `telemetry` is a
`TetrisTelemetryProviders` instance. Create fresh session-local variables and
tracker/provider instances for each concurrent episode; configuration and
telemetry providers may be shared. Explicitly force-flush and shut down the
shared telemetry after every episode has ended.

Choose metadata explicitly: live human `{dataset:"live", origin:"human",
clock:"wall"}`, live greedy `{dataset:"live", origin:"greedy", clock:"wall"}`,
or guided training `{dataset:"training", origin:"greedy", clock:"virtual"}`.
There is no dataset-derived clock inference. Use the real-time runner and
wall metadata for the live composition; keep the virtual runner for controlled
scenarios. Its provider retains actual contract/executable digests so callers
can distinguish received, queued, and applied authority.

An exposure ID is reserved before each request. Outcomes start only when the
selection actually applies; a locking outcome still belongs to the old exposure.
Completed slices cover at most five active game-clock seconds and exclude
paused time. Window IDs deduplicate gauge re-exports. Explicit zero-duration
completion markers establish episode coverage without adding quality weight.

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
| No active history or no locked pieces | Actual `current_drop_interval_ms` |
| Observations available and SDK-owned `_random < 0.5` | `200ms` |
| Observations available and `_random >= 0.5` | `1000ms` |
| No authored rule is eligible | Contract default `800ms` |

Runtime attributes state their temporal meaning explicitly:
`board_pressure_mean_5s`, `board_pressure_max_5s`,
`hard_drop_ratio_5s`, `placement_time_mean_ms_5s`, `pieces_locked_5s`,
`active_play_ms_5s`, `challenge_balance_5s`, `lines_cleared_5s`,
`game_over_count_5s`, `current_drop_interval_ms`, `session_id`, and `exposure_id`.
Level and rejected moves are not adaptive inputs. Results are absolute intervals
bounded to `200..1500ms` in `50ms` increments.

The no-data CEL predicate uses `active_play_ms_5s == 0.0` for the number field
and `pieces_locked_5s == 0` for the integer field. Learning uses duration-weighted
challenge balance, actual line clearing, game overs, and active time; placement
latency is not the reward. The learning policy evaluates every two seconds and
uses auto-activation.

Both examples package equal contract definitions, enforced by the application
contract test. Deployment manifests keep their files within their own directory
and retain different authorities (`integration` and `playground`), so their
immutable contract/executable digests differ. No legacy seven-field contract or
migration path remains.

The optional adapter uses `createDecisionClient` from `@flaggo/sdk/runtime`.
The deployment helper uses `bindDecisionContract` and `createContractClient`
from `@flaggo/sdk/management` after Contract and Decision Services are ready.
It injects the one manifest authority before its `deploy` operation sends the
authoritative `PUT`; it does not call the optional remote validation endpoint
first. The helper parses `flaggo.deploy/v2` and produces the immutable runtime
configuration from the accepted deployment.

## Automated checks

Run deterministic engine, provider, renderer, and terminal-cleanup tests:

```powershell
npm run test:tetris-app
```

Run the real-host integration independently:

```powershell
npm run test:tetris-integration
```

Run the credentialed live-agent extension separately:

```powershell
npm run test:tetris-analysis-integration
```

That command captures `gh auth token` in memory without printing or persisting
it, leaves `FLAGGO_ANALYSIS_MODEL` unset so the Copilot SDK selects its default,
and enables redacted per-session model/tool events in `async-analysis.log`.
The analysis fixture runs the same nine completed training episodes as the
playground: seven-bag seeds `11, 29, 53, 17, 37, 61, 13, 41, 67`, fixed
`3000ms` reaction, `200ms` command cadence, and a `120000ms` cap. Piece and
allocation streams are separate; gravity can overtake the player. Pressure,
hard-drop ratio, balance, lines, and game overs come from actual gameplay,
not assigned values or outcome delays derived from the returned interval.
After materialization, it requires the real agent to admit a distinct
Candidate and the normal auto-activation worker to make it authoritative. The harness
observes the registered `flaggo.analysis.cycle.completed` event and exact
contract, cycle, and Candidate correlations. `NoChange`, `NoCandidate`,
handoff, failure, timeout, and provider errors fail the test.
The test then verifies decisions and restart persistence use the activated
Candidate and checks no-data retention across several actual current intervals.
It also parses every structured live
`sessionEvent`, requires each serialized event to remain at or below 16 KiB,
and requires the broad real session to exercise `flaggoTruncation` metadata.
Because model behavior is intentionally nondeterministic, this is an opt-in
behavior-quality test rather than part of the deterministic application test
suite. Candidate delivery alone does not prove better gameplay; the playground
separately measures matched held-out improvement.

Successful runs remove their isolated directory. Failed runs print and
preserve its exact `.flaggo/t-*` path; inspect `async-analysis.log`, `w`, and
`c` to diagnose the full agent attempt.
The credential is not written to those artifacts.

The real-host harness runs two sustained greedy sessions with controlled
allocations to exercise both `200ms` and `1000ms` paths plus startup retention,
captures logs, metrics, and traces while forwarding them to the Rust OTLP
receiver, and checks resource/scope metadata, cross-signal correlation,
session isolation, exposure-attributed outcome windows, and deliberately
unmatched operational telemetry. A separate non-learning session checks
pause/resume/restart, line clearing, and rejected moves.
The two transport probes export `dataset=evaluation` gauges only to verify
durability; they are not held-out policy-quality evaluation and the learner
excludes them. The playground's actual held-out games still export no learning
evidence. Before application traffic, the harness sends minimal non-empty,
valid binary protobuf requests with identity and gzip transport compression to
each live `/v1/logs`, `/v1/metrics`, and `/v1/traces` endpoint. It verifies the
HTTP responses and later reads the retained batches through the production Raw
OTLP Inbox API to prove the complete six-case transport matrix was durably
stored.
The test compares the forwarded application request count and decompressed byte
total with OTel Ingestion's raw-inbox health, then restarts the hosts on the same
SQLite database and verifies those retained values survive reopening. Evidence
Materializer starts only after deployment, consumes every retained Raw OTLP
Inbox batch, and must create durable observations and origin provenance without
conflicts. Its structured startup and commit events must expose the expected
checkpoint, pending-batch lag, oldest pending age, newest evidence timestamp,
and evidence freshness. After stopping the writers, a Rust assertion reads
those rows through the production Evidence Store API and requires exactly the
four catalog-selected gauge sources plus all expected exact-digest built-in
decision observations. It verifies authority, source shape, materializer
versions, content digests, contract/executable provenance, rule results, and
session-and-exposure joins while rejecting any unselected source. It deduplicates
immutable windows, checks nonoverlap and exact game-clock duration, reconstructs
time-weighted balance and count totals against actual episode results, and
requires one completion marker per episode.
Materializer startup health after restart must then report the same Evidence
Store counts with no pending batches. The test also verifies SDK-owned
`_random`, direct REST parity from the captured session attributes,
retired-field absence, explicit failure after Decision Service stops, and
contract persistence across restart. Exporters are force-flushed before
assertions, so an export, receiver, materializer, semantic-evidence, or
durable-store failure fails the run.
