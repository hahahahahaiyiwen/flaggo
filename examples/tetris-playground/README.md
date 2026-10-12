# Flaggo Tetris playground

This playground demonstrates contextual gravity control with a dominant human
game and ten independently paced, real-time greedy players. Their attributed
evidence feeds production Copilot analysis through the real Flaggo lifecycle:

```text
Tetris decision
  -> application OTLP
  -> Raw OTLP Inbox
  -> Evidence Materializer
  -> Async Analysis
  -> Candidate admission
  -> auto-activation
  -> independently applied decisions in each live game
```

It runs locally without a cloud account. The browser owns no product state and
never reads SQLite, process logs, or analysis workspaces. A Node gateway owns
one human `TetrisSession`, ten continuous bot slots, application telemetry,
and session-local timers/providers. A separate benchmark launch owns the
controlled comparison workflow. Its observer queries only Loki, Tempo,
Prometheus, public service health endpoints, Contract Service management
endpoints, and launcher process state.

## Run

Install the repository prerequisites first:

- Docker Desktop or another Docker engine with Compose
- .NET 10 SDK
- the repository-pinned Rust toolchain
- Node.js and npm
- A GitHub token with Copilot access for the default live mode

From the repository root:

```powershell
npm ci
dotnet restore Flaggo.slnx --configfile NuGet.config
$env:FLAGGO_ANALYSIS_GITHUB_TOKEN = (gh auth token)
npm run tetris:playground
```

The launcher builds the required projects, allocates loopback-only ports,
starts the pinned observability containers and real Flaggo services, deploys
the playground contract, starts both background workers, then prints the
browser and Grafana URLs. Ports are dynamic; use the printed URLs rather than
assuming defaults.

Only the OpenTelemetry Collector and LGTM observability stack run in Docker.
The launcher builds and starts Rust binaries from `target\debug` directly and
C# services through `dotnet` with their `bin\Debug\net10.0` assemblies as local
child processes. The Node gateway and gameplay sessions also run on the host.
Flaggo's shared SQLite file lives in the run directory on the host; it is not
a database container. LGTM storage is bind-mounted from that directory.

The default mode is **live**, using the production Copilot analysis provider.
The token may instead come from `GH_TOKEN` or `GITHUB_TOKEN`; missing credentials
fail before starting the stack. Live mode does not silently substitute the
deterministic fitter.

For the separate, credential-free controlled benchmark:

```powershell
npm.cmd run tetris:playground -- --mode=benchmark
```

In Windows PowerShell, use `npm.cmd` when forwarding launcher options so the
`--` separator reaches npm.

Benchmark mode defaults to the example-owned deterministic `AgentProvider`.
Use `--mode=benchmark --analysis=copilot` for a production-agent benchmark.
Both providers use the real coordinator, immutable cutoff, bounded SQL tool,
workspace, proposal, Candidate admission, and activation worker. The benchmark
waits up to 45 seconds for deterministic activation or ten minutes for Copilot;
Copilot attempts have a ten-minute deadline, with a twenty-second limit per
evidence query. Live learning is ongoing and does
not promise a Candidate, a fixed activation time, or improved human enjoyment.

Live learning intentionally uses greedy-bot outcomes as a proxy for choosing
the human-facing policy; human comparison data is not required. Origin
contributions remain separately reported, not presented as measured human
preference. The agent may propose a narrow change when eligible comparisons
or an evidence-based inference support a positive expected duration-weighted
balance gain and expected satisfaction of both guardrails for the affected
population. Modest gains or ordinary sampling variability alone do not
require abstention.

The authored `200/400/600/800/1000ms` arms are initial exploration, not a Candidate
whitelist. Live Candidates may use any schema-valid interval from `200` to
`1500ms` in `50ms` steps, including previously unobserved `500/700ms`. An
inferred interval needs eligible supporting observations and explicit
interpolation, extrapolation, or gameplay-mechanism reasoning; linear response
must not be assumed. Findings distinguish measured results from predictions
and explain the expected objective effect, each guardrail, assumptions, and
uncertainty. These hypotheses may auto-activate before their own outcomes or
guardrail satisfaction are empirically verified; admission does not prove
safety. Missing direct observations of a proposed interval alone do not
require abstention. The benchmark contract remains observed-arm-only.

Balance is the fraction of active time inside the pressure band, not a
preference for the midpoint of the interval range; pressure anywhere inside the band scores
equally. Results are conditional on the fixed greedy-player pacing as well
as the objective, so an observed-arm winner is not a continuous optimum.

Eligibility still requires deduplicated, actually applied SDK exposures,
positive scored duration, and at least three distinct exposures per observed
supporting interval in each population a proposal changes. That minimum does
not require prior exposures for an unobserved proposed interval.
Startup/no-history selections are excluded, not history-bearing exploration
at `800ms`; actual terminal counts for
eligible exposures are retained. Contextual rules need matching
gameplay-context comparisons; a global choice
does not require arbitrary context splits. Aggregate guardrail success alone
does not justify `noChange`; unresolved eligibility, insufficient support for
an inference, material ambiguity in its rationale, or measured or predicted
guardrail violations for the proposed behavior still mean `noCandidate`.

Every query uses the cycle's immutable observation-time cutoff and captured
materialization watermark. Earlier authorized history remains available, but
an older observation materialized after snapshot capture waits for a later
cycle. The cutoff is not the time analysis completes.

## Play and observe

Use arrow keys to move, rotate, and soft-drop, Space to hard-drop, `P` to
pause, and `R` to restart. The application occupies the left half of the page.
The right half separates the control plane into equal decision delivery, data
collection, and data analysis lanes.

The ten bot slots initially show **Idle**. Use per-slot Start/Stop or
**Start all / Stop all**. Started slots continue through fresh episodes until
stopped; each game ends at actual game over or 120 seconds of active real-time
play. The gallery initially shows bots 1-3; each left/right arrow click moves
one slot while keeping three visible: 1-3, 2-4, through 8-10. The visible range
updates and controls disable at either boundary. Browsing changes rendering only: off-screen games keep
running, and Start all/Stop all and population summaries still cover all ten
slots. The miniatures do not shrink the human board. Collapse hides the
gallery and its paging controls without stopping gameplay.

Each slot owns its board, clock, piece stream, player, provider, and exposure
state. Human pause/restart affects only the human. Restart and normal bot
turnover create fresh session identities while retaining since-launch totals.
Unexpected bot failures remain visible and require an explicit per-slot Start;
Start all does not silently retry failed slots.

The session selector includes all ten bots independently of the gallery page
and changes inspection only; main controls and keyboard
always control the human. The decision delivery lane distinguishes the latest
received result, a queued result, and the applied interval/executable, with
evaluation source, latency, and explicit cached/fallback errors. It also
shows the full active executable plus retained active, Candidate, and inactive
executable history as a status/digest table. History comes from
`GET /v3/decision-contracts/{name}/versions/{digest}/executables/history`,
ordered by creation time and digest descending. The existing `/executables`
catalog still supplies active authority and pending Candidates. Both endpoints
include `nextCursor: null` on a final page, rather than omitting the required
field. The gateway
keeps one 50-record history page; **Older executables** and **Newest history**
navigate it while active authority remains visible. Select any row to open its rules,
provenance, and lifecycle details in a modal. A Candidate modal includes
**Activate** for a state-version-fenced authority change. A concurrent
authority or Candidate change is reported as a conflict rather than hidden as
success.

The table and open detail DOM remain intact across gameplay and poll-timestamp
updates when their displayed content has not changed, preserving clicks and
focus. Management catalogue/history reads have a five-second deadline; a failed
read retains cached details, shows an explicit error, and disables activation
until recovery.

The playable game starts with a fixed `800ms` interval. Level and line clearing
do not directly accelerate gravity. Policy refreshes occur every five seconds
while playing; a mid-piece result is queued and applies only after the piece
locks. The live gravity value is the actually applied interval, not a pending
result. A failed refresh retains the applied Flaggo selection and its exposure;
before a successful selection the fallback remains fixed, not level-based.

The human board (including the benchmark preview) and real-time bot episodes
use session-stable authored exploration. Each game computes SHA-256 of its
session ID, interprets the first four bytes as an unsigned big-endian integer,
and divides by `2^32` to configure the SDK's `random` callback. The value is
always in `[0, 1)` and remains the same for every evaluation in that game.
The SDK still injects `_random`; no new contract attribute or CEL function is
introduced. After the no-data rule, the live authored policy consistently
selects one of `200/400/600/800/1000ms`, using `_random` thresholds
`0.2/0.4/0.6/0.8`. Each arm has 20% allocation probability; ten bots average
two per arm, but assignment is not forced or guaranteed. Restart and bot
turnover create fresh IDs but may select the same arm. An early Candidate
can replace exploration before every arm has observations.
The benchmark preview retains its `200/1000ms` 50/50 authored policy.
Learned contextual policies remain free to adjust gravity within a game.
The guided virtual-clock comparison keeps its seeded per-decision allocation.

Human and bot summaries report active-time-weighted balance, lines/minute, and
game overs/minute, with each source's contribution shown separately. Ten
continuous bots will often supply most active minutes; that is not a claim of
improved human experience.

The data collection lane visualizes:

```text
OTLP Inbox -> Materializer -> EvidenceStore
```

Each stage shows current volume, backlog, or freshness from public readiness
and registered operational telemetry. Color intensity indicates relative
pressure; the gateway does not read the Evidence Store database.

The data analysis lane is a rolling analysis, Candidate, and activation
telemetry board. Each SSE update is timestamp-sorted so new events arrive at
the top, briefly highlight, and move older rows downward. The board retains
the newest 60 visible events. The **Not eligible** inclusion filter starts
unchecked; check it to include `analysis cycle completed: not_eligible`.
The other filters include or exclude their phases. Loki queries reserve
separate newest-activity samples with and without not-eligible cycles, and
the gateway reserves those histories independently of frequent decision and
ingestion events. A full buffer evicts older events, not new arrivals.
Backend HTTP calls have a two-second deadline; a stalled source is reported as
a backend failure rather than blocking subsequent observation indefinitely.
Successful Loki samples survive another sample's failure. The gateway merges
and deduplicates bounded last-known analysis activity across polls, without
caching failed metrics or readiness as successes. Partial or failed Loki/Tempo
collection explicitly shows **Stale / partial telemetry — retained history**
with the error and last complete poll time. A successful poll restores the
live indication; retained events keep their original observation timestamps.

### Inspect analysis actions

The analysis lane also exposes a read-only **Cycle / Attempt** selector and
expandable action rows. Selection includes the contract digest, so identically
named cycles from different contract versions do not merge. It remains pinned
as new cycles arrive. Expanded rows and keyboard focus survive unrelated
gameplay/SSE updates. Keyboard interaction in the inspection panel does not
send gameplay commands, and inspecting an attempt cannot alter runtime authority.

Trusted code emits `flaggo.analysis.action.started` and
`flaggo.analysis.action.completed` in the existing `flaggo.async-analysis`
scope, with `flaggo.analysis.action` spans for trace navigation. These travel
through the existing Collector/Loki/Tempo pipeline, not a new store. The
gateway reconstructs actions by **contract digest + cycle + attempt + UUID
action ID**. Each logical retry has a separate action ID. Exact duplicate
observations are ignored, arrival order is immaterial, and a terminal without
a start is displayed with **Start not observed**, not a fabricated timestamp.
Contradictory terminals produce **unknown/conflicting**, never chosen success.
An unmatched start says **completion not observed**: it does not prove that
work is still executing. A stale backend indication is separate from the
observed execution outcome, retains last-known facts, and clears on recovery.

Rows show only bounded, allowlisted facts: action kind/outcome, duration,
result rows/bytes, query/artifact digests, snapshot cutoff/watermark, hashed
provider session identity, bounded tool-call identity/failure category, and
trace links. They never expose SQL, prompts, datasets, artifact paths,
arbitrary summaries/errors, provider logs, or private reasoning. **Latest
observed action** is literal; absent provider activity is not “thinking”.
The current Copilot SDK does not reliably correlate concrete provider request
starts and terminals; `provider.request` is therefore absent rather than
fabricated. Other kinds appear only when trusted code observes their boundary:
`session.start`, `session.run`, `evidence.describe`, `evidence.cutoff`,
`evidence.sql`, `artifact.read`, `artifact.write`, `artifact.append`, and
`proposal.submit`. Provider custom-tool notifications do not duplicate the
trusted evidence/filesystem/proposal actions.

Discovery uses a separate newest-action Loki sample, protected from gameplay
log volume. The existing `/api/state` and SSE snapshots expose
`analysisActions`; selecting/refreshing an attempt issues
`GET /api/analysis/actions?contractDigest=…&cycleId=…&attemptId=…`.
That endpoint filters the supported Loki range API by all three correlation
fields. Automatic selected-attempt refresh is every five seconds. Queries
cover this playground run within at most the latest hour, request at most
400 events, and have the existing two-second backend deadline. The gateway
retains at most 400 validated action observations and 200 reconstructed
actions in memory; this is a bounded observation cache, not an authoritative
execution store. A reached query/history limit is explicitly shown as
**sample limit reached / retained history truncated**. Even below the limit,
missing telemetry and retention mean the view is not a complete timeline.
Evicted selections remain selected and can be refreshed, rather than silently
switching attempts. Scoped results are filtered before bounding, independently
of the global newest-action sample. The browser retains one selected-attempt
view with the same 400-observation/200-action limits, consumes the scoped
response directly, and merges matching SSE observations without letting newer
unrelated attempts evict the inspected history. Switching selection clears that
local view; a backend failure retains its last-known facts with explicit error
state.

Later concrete operations, such as Python evidence analysis or supervised
proposal review, should emit the same bounded facts at their actual trusted
boundaries when those operations exist. This playground adds neither
speculative agent roles nor a generic extensibility/event framework.

The header shows one health pill per service. Health derives from process
liveness and public readiness, not recent telemetry activity. Click a pill to
inspect process, readiness, last-signal age, and registered live metrics.
Sparse telemetry therefore remains informational and does not make a running,
ready service appear stale.

The timeline recognizes only names and attributes published by
`contracts/otel/flaggo-service-observability-profile-v1.json`. Unknown signals
are ignored rather than inferred. Backend query failures remain visible in the
gateway state and do not masquerade as empty successful responses.

## Run the adaptive comparison in benchmark mode

Launch with `--mode=benchmark`, then select **Run demo**. Benchmark mode has no
live bots; its playable preview exports neither learning outcomes nor SDK
learning receipts. Live mode rejects the comparison endpoint. The
single-flight workflow:

1. Measures the authored policy on three held-out games without exporting
   their learning evidence.
2. Runs nine training games using mixed seven-bag streams and the same fixed
   greedy player, exporting genuine decision receipts and outcome windows.
3. Force-flushes application OTLP and waits for materialization, analysis,
   Candidate admission, and activation of a distinct executable.
4. Measures that actually activated executable on the same held-out scenarios,
   again without exporting their learning evidence.
5. Completes only with at least five percentage points of challenge-balance
   improvement, at least 90% of baseline lines/minute, no additional game
   overs, and no reduction in total active survival time.

The visible board remains available for human play; these are separate
autonomous games, not scripted moves on the visible board. Comparison cards
show time-weighted balance, lines/minute, survived episodes, and executable
digests. Delivery success alone is not improvement. Insufficient data, no
Candidate, fallback, a mid-episode authority change, timeout, or a failed
comparison produces an explicit failed/inconclusive state.

The authored executable retains the current interval without observations,
then explores `200ms` and `1000ms` at 50/50 through SDK-owned `_random`.
Each episode has separate, repeatable piece and allocation streams. The player
does not change its placement heuristic or pacing with gravity.

| Protocol | Fixed value |
| --- | --- |
| Player | Current-piece greedy; reaction `3000ms`, command cadence `200ms` |
| Episode cap | `120000ms` of chronological virtual game time, or game over |
| Manageable pressure band | Settled-board-height pressure `0.15..0.45`, inclusive |
| Training seeds | `11, 29, 53, 17, 37, 61, 13, 41, 67` |
| Held-out seeds | `71, 89, 107`; never used by the learner |

The deterministic learner deduplicates immutable windows, requires matching
SDK receipts, nine completed training episodes, and at least three applied
exposures per arm in each proposed cohort. It evaluates pressure cuts and
optional hard-drop gates, selecting arms by observed active-time-weighted
balance rather than by interval size. Ambiguous or unsafe estimates produce
no Candidate. Predicted eligibility requires a 2.5-point gain, at least one
line/minute, and at most two game overs/minute; the separate held-out check
requires the stronger measured five-point improvement.

These calibrated constants describe one fixed model, not novice/expert
humans. Low pressure or frequent hard drops do not prove boredom, and higher
proxy balance does not prove human enjoyment. This iteration does not learn
a level curve, player strategy, or untested intervals.

Cancellation stops local gameplay but does not roll back legitimately
admitted or activated Candidates. A fresh authored baseline is required to
repeat the comparison; relaunch a clean playground after activation.

## Adaptive contract and attribution

`tetris.dropInterval` accepts twelve scalar fields: trailing pressure mean/max,
hard-drop ratio, placement time, locked pieces, active duration, challenge
balance, cleared lines, game-over count, the current applied interval, session
ID, and prospective exposure ID. Result bounds remain `200..1500ms` in `50ms`
increments, with default `800ms`. The Flaggo terminal and integration harness
use the benchmark contract definition. The live definition preserves the same
online interface, no-history rule, evidence bindings, objective, and activation
policy, but expands authored exploration to five equally weighted arms and
changes learning guidance. The adapter, experience tracker, and
fixed gameplay protocol live in `tetris-integration`; this playground imports
them. Separate deployment authorities still produce different immutable
digests. Standalone terminal play retains its local level-based policy.

The application reserves an exposure ID before a request and starts its outcome
window only when the result is applied. Discarded or cancelled pending results
receive no outcomes. Completed slices are at most five active seconds, end at
policy replacement or lifecycle boundaries, and retain an immutable window ID,
game-clock bounds, actual interval, dataset, origin, clock, and before-call context.
Zero-duration completion facts establish episode coverage without adding quality
weight. Paused time is excluded.

The shared Tetris metric reader uses delta collection at the existing
five-second export cadence: only newly recorded completed-window points are
exported, rather than repeatedly exporting historical windows. No windows or
measurements are sampled. The existing bounded OTLP retries and shutdown flush
remain; terminally failed collections are not replayed on later collections,
and client transport is not durable.

The `tetris.experience` scope emits four correlated gauges:
`challenge_balance`, `lines_cleared`, `game_over_count`, and `active_play_ms`,
each prefixed with `tetris.experience.`. Correlation uses both session and
exposure IDs. Before-call summaries are features; only after-application
outcomes are rewards. Balance is weighted by active time, count rates use that
same denominator, and re-exported windows are not new observations.

| Source | Dataset | Origin | Clock |
| --- | --- | --- | --- |
| Live human | `live` | `human` | `wall` |
| Live bots | `live` | `greedy` | `wall` |
| Guided training | `training` | `greedy` | `virtual` |

The live contract analyzes completed windows from ongoing games, not just
completed episodes. Guidance requires actual receipt joins, immutable
before-call context, and at least three distinct scored applied exposures per
observed arm/context used to justify a change. It excludes no-history
selections, weights by active duration, and reports origins separately. These
are analysis instructions, not additional platform admission guarantees.
Insufficient or unsafe evidence can legitimately produce **No Candidate**.
There is no new exploration scheduler or hidden baseline reset after activation.

`flaggo.live.deploy.json` selects
`flaggo/contracts/live/tetris.dropInterval.decision-contract.json`;
`flaggo.deploy.json` retains the guided definition. Each launch has its own
stack/store and immutable contract version, so live evidence and activation
cannot contaminate the benchmark.

## Telemetry authority

`compose.yaml` pins Grafana LGTM and a separate OpenTelemetry Collector. The
Collector exposes two OTLP/HTTP receivers:

| Pipeline | Destination |
| --- | --- |
| Application (`TetrisSession`, `tetris.policy`, `tetris.experience`, `@flaggo/sdk`) | OTel Ingestion and LGTM |
| Internal Flaggo service telemetry | LGTM only |

This split prevents internal operational telemetry from becoming product
evidence and prevents OTel Ingestion from recursively ingesting its own
telemetry. The gateway injects W3C `traceparent` into Decision Service
requests so application policy spans correlate with service spans.

## Cleanup and retained diagnostics

Cleanup awaits hosts in reverse startup order: the gateway finishes local
sessions and telemetry shutdown before its service and observability providers
stop. Failures do not prevent the remaining hosts from being stopped.
Cleanup diagnostics include every AggregateError child and nested cause,
including the host name or filesystem error instead of only the outer stack.

Ctrl+C stops the gateway, workers, HTTP services, Collector, and LGTM in
reverse dependency order. The unique Compose project, volumes, database, and
run directory are then removed.

Use `--retain` to preserve the run directory and LGTM bind-mounted data:

```powershell
npm.cmd run tetris:playground -- --retain
```

The launcher prints the retained `.flaggo\tetris-playground-*` path. It
contains one log per process, the isolated SQLite database, analysis
workspaces, and LGTM data. No other directory is removed.

If startup fails:

1. Confirm `docker info` succeeds and Docker uses Linux containers.
2. Inspect `observability.log` for Collector or LGTM configuration errors.
3. Inspect the named service log for process startup or readiness errors.
4. Relaunch without `--retain` when a clean authored baseline is required.

## Checks

Run shared contract, experience, provider, and terminal tests, then the
playground comparison, gateway, observer, and projection tests:

```powershell
npm run test:tetris-app
npm run test:tetris-playground
```

The browser fixture covers the dominant human board, single-slot browsing
across ten miniatures,
session-local inspection, keyboard ownership, duration weighting, executable inspection and
activation, health details, collection visualization, rolling telemetry,
cycle/attempt action selection, focus-preserving expansion, partial/conflicting
actions, backend outage/recovery, and
pending/successful/inconclusive comparison cards. The Docker-backed end-to-end
test exercises the pinned containers and real services, checks actual held-out
improvement and executable authority, attaches the evaluation snapshot, and
verifies cleanup. The credentialed live smoke uses actual mixed-source wall-clock
windows, receipt joins, and trusted production Copilot SQL action metadata; it does not require
a Candidate or validate human preferences. It collects one minute of legal
play, stops the bots, and leaves the human game paused or game over while
shared analysis continues.
The ten-minute observation budget is a test timeout, not a live latency guarantee.
It attaches evidence and per-cycle bounded SQL action/completion metadata,
without reading raw provider session logs or SQL result payloads. Action
telemetry does not prove which result origins were returned: mixed origins
are verified separately at the existing test-only evidence boundary.
That smoke requires Node.js 22.13+ for the
built-in `node:sqlite` module.

```powershell
npm run test:tetris-playground:browser
npm run test:tetris-playground:e2e
$env:FLAGGO_ANALYSIS_GITHUB_TOKEN = (gh auth token)
npm run test:tetris-playground:live
cargo test --locked -p flaggo-tetris-playground-analysis
```

### Phase 4 exit verification

The cloud-free exit check uses the deterministic benchmark, not hosted Copilot:

```powershell
npm.cmd run test:tetris-playground:e2e -- --repeat-each=2
```

Each repetition launches a fresh isolated stack, learns and auto-activates a
distinct executable, verifies matched held-out gameplay, and checks SDK/direct
REST parity for two scored contexts. It verifies exact contract/Candidate
identity, cycle/attempt provenance, immutable cutoff and watermark, and
registered metrics from every service. Bounded Loki queries and exact Tempo
trace reads verify log/trace correlation from all five services. A rejected
OTLP media type and an activation state conflict exercise representative
failure telemetry without replacing active authority.

The test discovers its own host addresses from startup records and its own
Compose port bindings; semantic assertions use public runtime, management,
and telemetry-backend APIs, not direct database access. Attachments include
the terminal snapshot, final `adaptive-evaluation.json` with runtime
configuration and parity responses, and `service-telemetry.json`. Failed runs
also attach bounded analysis diagnostics. Shutdown must exit normally, make
the gateway unreachable, and remove the test's specific run directory before
the next repetition. User-retained runs in other worktrees are untouched.

On 2026-10-11, both clean repetitions passed with the following fixed identities:

| Artifact | Digest |
| --- | --- |
| Deployed benchmark contract | `sha256:f276a971660fad5bf88f6b0149652012b5761b9d9ba14fc811cbbb727a9d0457` |
| Authored baseline | `sha256:a10d8890f38c0d750727e47228654504660920fd3ed4025e60e02522bac7fbfa` |
| Admitted and activated learned executable | `sha256:20ea5398cc29820219bb2327f75185b83975a947b3a482f071113f58d2b79af8` |

The successful cycles were `8bb9ee4b-15c2-4c05-8111-019d170dfc10` and
`8ab00dd1-531f-4ef2-a476-25d482c5dd57`, with cutoffs
`2026-10-11T22:20:19.5967624Z` and `2026-10-11T22:22:06.2070213Z`;
both captured watermark `784`. The local launcher configuration is defined in
`local-stack.mjs`: materializer polling `25ms`, catalog refresh `1s`, analysis
polling `250ms`, one agent, Contract Service activation polling `250ms`, and
internal metric export `500ms`. Application OTLP collection remains `5s`.

| Matched held-out measure | Baseline | Learned |
| --- | --- | --- |
| Active-time-weighted balance | 52.46% | 84.83% |
| Lines/minute | 1.54 | 4.67 |
| Active survival time | 194800ms | 360000ms |
| Game overs in three episodes | 3 | 0 |

This is a 32.37-percentage-point measured gain under the fixed greedy-player
protocol, not proof of human enjoyment or a continuous optimum.

The approved export-failure requirement is diagnostics and failure
isolation/recovery, not exact browser exporter status. Application entrypoints
enable the standard OTel console logger at ERROR level; gateway errors are
captured in `playground-gateway.log`, independently of the failing OTLP path.
The terminal and real-host integration entrypoints use the same diagnostic
configuration. The browser shows measured backlog/freshness, failed or
superseded analysis, rejected activation, stale backend collection, and
recovery without inferring client transport success or failure.

`tetris-integration/test/otel.test.mjs` injects a non-retryable HTTP `400` at a
local metric export boundary and verifies actual standard console diagnostics.
The same test exercises the real Tetris session and SDK/provider with a fixed
HTTP decision response: gameplay and policy refresh continue, the applied
executable/interval remain unchanged, and no management/activation request is
made. After transport recovery, new completed windows export successfully,
without replaying the failed delta window. This is an application-boundary
failure test, not a failure injected into the real-service browser run; the
real-service authority and activation checks above remain separate.

An ingestion `415` or stale Loki query is not terminal client-export evidence.
Production authentication, durable client export replay, statistical safety,
and rollout control remain outside this exit verification. Bounded
agent-action observability is verified separately without increasing those
guarantees.
