# Flaggo Async Analysis

The Async Analysis Service continuously turns materialized evidence into
validated, inactive Candidate executables. It is a background worker and never
participates in runtime decisions.

The local implementation combines:

- a provider-neutral coordinator and bounded Agent Pool;
- one durable, name-keyed workspace with an exclusive process and OS lock;
- one immutable exact contract digest, evidence cutoff, and Evidence Store
  watermark per cycle;
- read-only, contract-scoped, bounded SQL over the Evidence Store;
- a Copilot SDK adapter running the local Copilot CLI in empty mode;
- an attempt-scoped virtual POSIX filesystem and versioned
  `evidence-analysis` skill; and
- Contract Service Candidate admission with current-digest fencing.

## Module boundaries

| Crate | Ownership |
| --- | --- |
| `flaggo-analysis-domain` | Stable analysis values, errors, and pure cadence calculations without provider dependencies |
| `flaggo-analysis-workspace` | Workspace contracts, durable cycle semantics, and the local filesystem provider |
| `flaggo-analysis-agent` | Provider-neutral agent/session contracts, bounded Agent Pool, and the optional empty-mode Copilot adapter |
| `flaggo-async-analysis` | Coordinator policy, configuration, Contract Service and Evidence Store adapters, and the process entrypoint |

Workspace code cannot discover contracts, query evidence, invoke an agent, or
submit a Candidate. Agent code, including its Copilot submodule, cannot
schedule cycles or mutate workspace control state. The Copilot adapter
translates declared capabilities but does not own analysis policy. The app
library owns coordination and local external-system adapters; its binary is
the composition root that selects concrete providers.

The agent cannot access a shell, host paths, arbitrary network endpoints,
database credentials, activation operations, or Executable Store tables.
`propose_executable` can persist one validated `candidate` lifecycle record;
activation remains a separate Contract Service concern.
The headless permission predicate approves only filesystem reads/writes and
the five registered analysis tools. `check_analysis_status` reads only the
coordinator-owned control signal; the agent does not query Contract Service to
detect supersession. The permission predicate rejects shell, URL, MCP, memory,
hook, unknown-permission, and unknown-custom-tool requests; the virtual
filesystem still enforces the narrower workspace read/write paths.

## Run locally

The Evidence Store schema must already exist in `FLAGGO_DATABASE_URL`, Contract
Service must be reachable, and the local Copilot CLI must be installed and
authenticated. When `FLAGGO_ANALYSIS_MODEL` is absent, the Copilot SDK selects
its default model:

```powershell
cargo run -p flaggo-async-analysis
```

Set `FLAGGO_ANALYSIS_MODEL` only when a run must pin one exact model. By
default the SDK authenticates as the user already logged in through `gh`. A
test can capture that credential without printing it and pass it only to the
child process:

```powershell
$env:FLAGGO_ANALYSIS_GITHUB_TOKEN = gh auth token
$env:FLAGGO_ANALYSIS_LOG_SESSION_EVENTS = 'true'
try {
  cargo run -p flaggo-async-analysis
} finally {
  Remove-Item Env:\FLAGGO_ANALYSIS_GITHUB_TOKEN -ErrorAction SilentlyContinue
  Remove-Item Env:\FLAGGO_ANALYSIS_LOG_SESSION_EVENTS -ErrorAction SilentlyContinue
}
```

The token is held in process memory and is never included in structured
startup or session logs. Session-event logging is opt-in because assistant
messages and tool payloads can contain analysis evidence. Sensitive-key values
are redacted before events are emitted as
`async_analysis.agent_session_event`. Each `sessionEvent` is capped at 16 KiB;
truncated events include `flaggoTruncation` metadata and retain identifying
fields plus a bounded diagnostic preview.

| Setting | Default | Purpose |
| --- | --- | --- |
| `FLAGGO_DATABASE_URL` | `sqlite://flaggo.db` | Read-only Evidence Store database |
| `FLAGGO_CONTRACT_SERVICE_URL` | `http://127.0.0.1:5000` | Base URL for current-contract reads and Candidate admission |
| `FLAGGO_CONTRACT_CATALOG_URL` | derived from Contract Service | Optional full current-catalog endpoint URL |
| `FLAGGO_ANALYSIS_WORKSPACE_ROOT` | `./flaggo-analysis-workspaces` | Durable contract workspaces |
| `FLAGGO_ANALYSIS_COPILOT_HOME` | `./flaggo-analysis-copilot` | Isolated local Copilot runtime state |
| `FLAGGO_ANALYSIS_MODEL` | Copilot SDK default | Optional exact Copilot model ID |
| `FLAGGO_ANALYSIS_GITHUB_TOKEN` | logged-in `gh` user | Optional in-memory GitHub credential |
| `FLAGGO_ANALYSIS_LOG_SESSION_EVENTS` | `false` | Emit redacted model and tool session events |
| `FLAGGO_ANALYSIS_MAX_AGENTS` | `1` | Concurrent worker and agent-session limit |
| `FLAGGO_ANALYSIS_POLL_INTERVAL_MS` | `5000` | Catalog, eligibility, and supersession polling interval |
| `FLAGGO_ANALYSIS_QUERY_MAX_ROWS` | `1000` | Maximum rows returned by one evidence query |
| `FLAGGO_ANALYSIS_QUERY_MAX_BYTES` | `1048576` | Maximum encoded result bytes per evidence query |
| `FLAGGO_ANALYSIS_QUERY_TIMEOUT_MS` | `10000` | Evidence-query and local HTTP timeout |
| `FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS` | `1200` | Maximum Copilot attempt duration |
| `FLAGGO_ANALYSIS_YIELD_GRACE_SECONDS` | `30` | Checkpoint grace period for supersession and shutdown |

## Operational observability

The process uses service name `flaggo-async-analysis` and instrumentation
scope `flaggo.async-analysis`. It emits registered service lifecycle events,
one root `flaggo.analysis.cycle` span for each exact-contract work attempt,
bounded cycle outcome and duration metrics, and an active-worker gauge. Exact
contract, cycle, attempt, and Candidate identifiers appear only on spans and
structured lifecycle logs; they are never metric dimensions.

Console logs are always available. Logs, metrics, and traces export as
OTLP/HTTP binary Protobuf when the standard
`OTEL_EXPORTER_OTLP_*_ENDPOINT` variables are configured. Invalid exporter
configuration fails startup, exporter unavailability does not change analysis
outcomes, and shutdown performs a bounded provider flush. Prompts, evidence,
model output, credentials, and workspace content are excluded from service
telemetry. Opt-in `async_analysis.agent_session_event` diagnostics remain
direct bounded console output and are not sent through the service's OTLP
pipeline.

## Durable cycle behavior

The physical workspace directory is the SHA-256 digest of the contract name;
the manifest retains the original name. Completed cycles are append-only.
Recoverable provider, tool, HTTP, and model failures keep the current cycle
open so a fresh agent session can resume from durable artifacts.

Candidate proposal is journaled before submission. A retry reuses the original
attempt identity and exact rules, allowing safe recovery if the process loses
the HTTP response. A different second proposal is rejected. Analysis artifacts
are sealed into a content-digested manifest before admission; mutable host
control records and Copilot session state are excluded. The manifest records
the exact analysis skill name and version used by the cycle.

The agent sees only the active cycle at `/workspace`. It may write beneath
`/workspace/analysis` and `/workspace/handoffs`; host-owned cycle records are
read-only. Completed prior cycles are projected read-only beneath
`/workspace/history/<cycle-id>`. Provider runtime state is isolated beneath
`/workspace/analysis/.copilot` and is excluded from durable analysis
provenance.

Each evidence scope also includes exact-contract-digest
`@flaggo/sdk` / `flaggo.decision.received` observations. They expose the
decision ID, active executable digest, serialized result and hash, evaluation
source/rule, request correlation ID, and explicit correlation attributes.
They do not expose the active executable body or the complete decision input
unless the application supplied those values as correlation attributes.
`describe_evidence` publishes the normalized decision-attribute and metric
data-point JSON paths. Finite OTel doubles retain their exact bit encoding and
also expose a decimal `value` for SQL analysis. Metric observations may be
cumulative snapshots, so analysis must use the latest point or changes between
points for each correlated population rather than summing repeated exports.

The first cycle becomes eligible at `acceptedAt + evaluate.interval`. Later
cycles become eligible at the prior terminal cycle's
`completedAt + evaluate.interval`. A new current digest supersedes an active
old cycle. The coordinator is the sole supersession detector: it marks the
cycle superseding, raises the agent-visible control signal, blocks further
trusted analysis operations, and grants a bounded period for the agent to
write `/workspace/analysis/supersession.md` and acknowledge the signal. The
coordinator finalizes supersession even if the agent does not cooperate and
rechecks currentness before accepting an ordinary terminal result. Contract
Service independently provides the final atomic stale-digest fence for
Candidate admission. The replacement digest receives its own schedule.
Terminal `outcome.json` is the completion journal: restart reconciliation
finishes cycle status, cadence, and current-pointer updates idempotently.
`NoChange` is a successful terminal outcome when completed analysis supports
retaining the current executable; `NoCandidate` means available evidence is
still insufficient to select a Candidate.

## Validation

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run test:tetris-integration
npm run test:tetris-analysis-integration
```

The last command is an opt-in credential/model-dependent behavior-quality
test. It requires an authenticated GitHub CLI and fails unless the real agent
persists an inactive Candidate.
