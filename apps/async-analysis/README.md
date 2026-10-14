# Flaggo Async Analysis

The Async Analysis Service continuously turns materialized evidence into
validated, inactive Candidate executables. It is a background worker and never
participates in runtime decisions.

The local implementation combines:

- a provider-neutral coordinator and bounded Agent Pool;
- one durable, name-keyed workspace with an exclusive process and OS lock;
- one immutable exact contract digest per cycle, with live evidence queries
  and persisted datasets;
- read-only, contract-scoped, bounded SQL over the Evidence Store;
- SQL-to-file results and a fresh-process local Python modeling tool;
- a Copilot SDK adapter running the local Copilot CLI in empty mode;
- module-owned role/task prompts, composable unversioned skills, and an
  attempt-scoped virtual POSIX filesystem; and
- Contract Service Candidate admission with current-digest fencing.

## Module boundaries

| Crate | Ownership |
| --- | --- |
| `flaggo-analysis-domain` | Stable analysis values, errors, and pure cadence calculations without provider dependencies |
| `flaggo-analysis-workspace` | Workspace contracts, durable cycle semantics, and the local filesystem provider |
| `flaggo-analysis-agent` | Static agent context, provider-neutral agent/session contracts, bounded Agent Pool, and the optional empty-mode Copilot adapter |
| `flaggo-async-analysis` | Coordinator policy, configuration, Contract Service and Evidence Store adapters, and the process entrypoint |

Workspace code cannot discover contracts, query evidence, invoke an agent, or
submit a Candidate. Agent code, including its Copilot submodule, cannot
schedule cycles or mutate workspace control state. The Copilot adapter
translates declared capabilities but does not own analysis policy. The app
library owns coordination and local external-system adapters; its binary is
the composition root that selects concrete providers.

## Agent context

The agent-facing surface is intentionally split by responsibility:

| Surface | Location | Responsibility |
| --- | --- | --- |
| Role and mission | `modules/analysis-agent/agent/prompts/system.md` | Define the data-scientist role, closed-loop stages, contract objective, guardrails, and executable mission without encoding workflow |
| Minimal task | `modules/analysis-agent/agent/prompts/task.md` | Require `analysis-cycle-protocol`; `context.rs` binds the dynamic contract, workspace, cycle, and attempt identity |
| Cycle protocol | `modules/analysis-agent/agent/skills/analysis-cycle-protocol/SKILL.md` | Cycle identity/inputs, work-product/history discovery, resumption, status/checkpointing and attempt outcomes |
| Contract interpretation | `modules/analysis-agent/agent/skills/understand-decision-contract/SKILL.md` | Interpret the accepted contract wrapper, learning objective, guardrails, evidence mappings, and executable constraints |
| Analytical method | `modules/analysis-agent/agent/skills/qualitative-analysis/SKILL.md` | SQL/Python method choice, sampling/data quality, population and temporal boundaries, comparative reasoning, uncertainty and the executable conclusion |
| Executable authoring | `modules/analysis-agent/agent/skills/author-executable/SKILL.md` | Apply the supported `flaggo.cel/v1` profile and convert supported analysis into one minimal executable proposal |
| Writable artifact templates | `modules/analysis-agent/agent/templates` | Provide existing files that the runtime's edit tool can update |
| Tool descriptions and permission allowlist | `modules/analysis-agent/src/copilot/tools.rs` | Six built-ins and five typed tools; SQL result semantics and Python invocation/results, with SDK-native file creation/editing |
| Trusted tool behavior | `src/local_capabilities.rs` | Coordinator status, live scoped SQL-to-file results, local Python execution and inactive Candidate admission |
| Operational guidance | This README | Deployment/configuration, trusted-local limitations, resource controls and recovery guarantees |

Every prompt, skill, and template uses Markdown headings for human-readable
hierarchy and semantic XML tags around each agent-facing instruction or
artifact section. Skill files retain YAML `name` and `description` frontmatter
only for Copilot discovery.

The durable analysis profile binds each cycle to the ordered skill names
`analysis-cycle-protocol`, `understand-decision-contract`,
`qualitative-analysis`, and `author-executable`; skills are deliberately
unversioned.
`modules/analysis-agent/src/context.rs` owns the assets, binds dynamic cycle
identity as escaped XML, and prepares the writable templates. The current
local packaging loads skills from the analysis-agent module source tree.

The SDK-native `create` and `edit` tools create files and make incremental
edits through the workspace filesystem adapter, which enforces writable
locations. Native `create` requires an existing parent directory; inline
Python can create subdirectories when needed. Before a session starts,
the analysis-agent context package also creates stable
findings, supersession, and handoff templates inside the writable workspace.
The agent preserves their semantic XML tags and replaces their Markdown
placeholders; resumed attempts retain existing content.

The Copilot built-in tool catalog does not expose a shell, host paths,
arbitrary network endpoints, database credentials, or activation operations.
However, `code_interpreter` deliberately runs unsandboxed Python under the
local OS account, with host filesystem/network/subprocess access and the
inherited environment. Its working directory and agent instructions guide
usage; they do not enforce confinement. This is a trusted-local prototype,
not a sandbox. Python can bypass virtual filesystem/sealing restrictions
and can access existing services; no runtime-authority isolation is claimed.
`propose_executable` can persist one validated `candidate` lifecycle record;
activation remains a separate Contract Service concern.
The headless permission predicate approves only filesystem reads/writes and
the five registered analysis tools: `check_cycle_status`, `describe`,
`run_sql`, `code_interpreter`, and
`propose_executable`.
`check_cycle_status` reads only the coordinator-owned control signal; the
agent does not query Contract Service to detect supersession. The permission
predicate rejects shell, URL, MCP, memory, hook, unknown-permission, and
unknown-custom-tool requests; the virtual filesystem still enforces the
narrower workspace read/write paths.

## Local query and Python workflow

1. Call `describe` to discover store-owned logical relations, columns/types and
   SQL capabilities, plus service query limits and the current trusted scope.
2. Call `run_sql` with SQL and optional `row_limit`. Each call executes one
   scoped query, saves JSON under `/workspace/analysis/queries/<query-id>.json`,
   and appends query history at `/workspace/analysis/queries/history.jsonl`.
   The response contains `queryId`, `resultPath`, `historyPath`, `columns`, `savedRows`,
   `totalRows`, `truncated`, `rowLimit`, file `resultBytes`, `contentDigest`,
   `queryStartedAt`, and `queryCompletedAt` -- no inline rows. `resultPath` and `historyPath`
   are relative to the cycle analysis directory. Read rows from the file's
   `rows` array. The default limit is `min(100, service ceiling)`; explicit
   limits must be positive and at most the ceiling. One extra row detects
   truncation without a COUNT query; `totalRows` is null when truncated and
   equals `savedRows` otherwise. Use `ORDER BY` for stable sampling. An
   intentional SQL `LIMIT` describes that query's result, not the full source
   population. `columns` contains `{name, type}` descriptors for the actual
   query output, including aliases and empty results; unknown types are null.
   Duplicate output names are rejected instead of silently overwriting values.
3. Use the SDK-native `create` tool with an absolute virtual `path`, such as
   `/workspace/analysis/model.py`, and UTF-8 `file_text`.
   Use `edit` to change an existing file.
4. Call `code_interpreter` with exactly one of `script_path` or inline `code`.
   File paths may be relative, such as `model.py`, or absolute
   virtual paths under `/workspace/analysis`. Inline source is saved directly
   as the execution's submitted-code artifact; it needs no separate file
   creation call. Both forms execute a saved copy using the configured Python
   executable, directly without a
   shell. The working directory is the cycle analysis directory; use relative
   paths such as `queries/<query-id>.json`, not `/workspace/...` inside Python.
   The original script directory (or the analysis directory for inline code)
   is available for imports, while `__file__`
   identifies the saved execution copy. Each invocation has fresh memory.

SQL validation, source/authority/contract scope, byte limits
and query timeout remain enforced by the trusted SQL capability. Row overflow
is now explicit successful truncation; byte/time failures do not return a
successful partial artifact. History and trusted audit entries refer to
saved results rather than duplicating their rows. Save/history/audit errors
remain explicit; interrupted multi-file persistence can leave an orphan file,
not a successful result or an automatically retried query.

Queries read live evidence: no cycle-wide timestamp cutoff or ingestion
watermark is imposed. The agent selects time predicates and an appropriate lag,
accounts for materialization delay, and can deliberately refresh a saved dataset.
Query start/completion times describe execution, not evidence completeness.
Saved files are stable inputs for a modeling run; subsequent queries or resumed
attempts can see newly materialized observations. Existing historical
`cutoff.json` files are not used to constrain new queries. Candidate provenance
references the sealed analysis manifest rather than a claimed frozen snapshot.

Python responses contain an execution ID, submitted-code and execution-record
paths, observed runtime metadata, and an `execution` object with status,
exit code, elapsed milliseconds, bounded stdout/stderr, truncation flags,
`outputComplete`, and an optional diagnostic. Statuses are `succeeded`,
`failed`, `launchFailed`, `timedOut`, and `cancelled`. Runner I/O/persistence
errors remain tool errors with a failure record when persistence succeeds.
Records live under `analysis/executions/<execution-id>` and include the
submitted code/digest, configured interpreter, available actual runtime
identity, and outcome. Generated files stay where the script writes them.
Existing results/scripts/records survive ordinary restart and handoff; the
runtime does not implicitly replay executions.

The default interpreter is the source checkout's root `.venv`: `Scripts/python.exe`
on Windows or `bin/python` on Linux/macOS. Set `FLAGGO_ANALYSIS_PYTHON` to
override it; there is no automatic fallback to system Python. Provision packages
outside agent-generated code. The tracked `requirements.txt` includes NumPy,
pandas, SciPy, scikit-learn, matplotlib and seaborn; package versions are not
pinned or allowlisted. Generated virtual environments are ignored by Git.
Large data, remote storage, persistent notebook kernels, hard memory/CPU/
artifact quotas, sandboxing and complete input/output provenance are deferred.
Timeout/cancellation kills and waits for the direct Python process only;
descendants are not contained. Pipe draining is bounded after process exit,
and `outputComplete: false` explicitly reports unfinished capture. Output
limits bound captured bytes per stream, not generated files or computation.
Host access means recorded artifacts are not tamper-proof.

## Run locally

Install Python 3 and prepare the analysis environment from the repository root:

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r .\apps\async-analysis\requirements.txt
```

On Linux/macOS, use `.venv/bin/python` for the install command. Recreate the
environment in each checkout rather than copying or committing it. The service
uses the virtual environment by default without activation or setting an
environment variable; its location is bound to the source checkout at build
time, like the current skill-directory packaging.

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
try {
  cargo run -p flaggo-async-analysis
} finally {
  Remove-Item Env:\FLAGGO_ANALYSIS_GITHUB_TOKEN -ErrorAction SilentlyContinue
}
```

The token is held in process memory and is never included in structured
startup or action logs. Analysis progress uses allowlisted action metadata
through standard service telemetry, not raw provider session messages or
tool payloads. There is no separate session-event logging switch.

| Setting | Default | Purpose |
| --- | --- | --- |
| `FLAGGO_DATABASE_URL` | `sqlite://flaggo.db` | Read-only Evidence Store database |
| `FLAGGO_CONTRACT_SERVICE_URL` | `http://127.0.0.1:5000` | Base URL for current-contract reads and Candidate admission |
| `FLAGGO_CONTRACT_CATALOG_URL` | derived from Contract Service | Optional full current-catalog endpoint URL |
| `FLAGGO_ANALYSIS_WORKSPACE_ROOT` | `./flaggo-analysis-workspaces` | Durable contract workspaces |
| `FLAGGO_ANALYSIS_COPILOT_HOME` | `./flaggo-analysis-copilot` | Isolated local Copilot runtime state |
| `FLAGGO_ANALYSIS_MODEL` | Copilot SDK default | Optional exact Copilot model ID |
| `FLAGGO_ANALYSIS_GITHUB_TOKEN` | logged-in `gh` user | Optional in-memory GitHub credential |
| `FLAGGO_ANALYSIS_MAX_AGENTS` | `1` | Concurrent worker and agent-session limit |
| `FLAGGO_ANALYSIS_POLL_INTERVAL_MS` | `5000` | Catalog, eligibility, and supersession polling interval |
| `FLAGGO_ANALYSIS_QUERY_MAX_ROWS` | `1000` | Maximum rows returned by one evidence query |
| `FLAGGO_ANALYSIS_QUERY_MAX_BYTES` | `1048576` | Maximum encoded result bytes per evidence query |
| `FLAGGO_ANALYSIS_QUERY_TIMEOUT_MS` | `20000` | Evidence-query and local HTTP timeout |
| `FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS` | `1200` | Maximum Copilot attempt duration |
| `FLAGGO_ANALYSIS_YIELD_GRACE_SECONDS` | `30` | Checkpoint grace period for supersession and shutdown |
| `FLAGGO_ANALYSIS_PYTHON` | Checkout root `.venv` interpreter | Optional override; multi-component relative paths resolve at startup |
| `FLAGGO_ANALYSIS_PYTHON_TIMEOUT_SECONDS` | `60` | Direct Python process deadline |
| `FLAGGO_ANALYSIS_PYTHON_MAX_OUTPUT_BYTES` | `16384` | Captured bytes per stdout/stderr stream; excess is drained and discarded |

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
telemetry.

### Observed action progress

`flaggo.analysis.action.started` and `flaggo.analysis.action.completed` logs
describe meaningful operations inside an attempt; `flaggo.analysis.action`
spans provide measured duration and trace correlation. Session start/run,
evidence discovery/SQL, Python execution, artifact inspection/edits and proposal submission
share one provider-neutral vocabulary. Deterministic providers use the same
trusted capability boundaries as Copilot. Provider-only observations are
reported only when supported by actual SDK signals. The current Copilot SDK
does not supply reliably paired concrete request start/terminal signals, so
`provider.request` timing is unavailable. Session activity or a gap between
tools must not be displayed as measured model waiting.

Each invocation has a generated action UUID and exact contract/cycle/attempt
correlation. A failed query and a later retry remain separate actions. SQL
details expose a digest, duration, and actual
returned rows/encoded bytes, never SQL or result data. Artifact references use
content digests and sizes instead of paths/content; append describes the
appended fragment, not the resulting full file. Internal `.copilot`/`.git`
content accesses and metadata-only operations are excluded. Result rows are not scanned
observations.

The registered field and outcome allowlists are defined by
[internal service observability](../../docs/design/architecture/OBSERVABILITY.md)
and its machine-readable profile. Action metadata and identities are not
metric dimensions. No private reasoning, model text, arbitrary free-text
summaries or provider payload mirror is emitted.

### Troubleshooting progress

In the playground, inspect the selected cycle and attempt before interpreting
the latest action. An expensive SQL query can return a small result: measured
duration and returned bytes describe different things. Explicit timeout/error
observations remain visible even if the next query succeeds.

A session with no recent tool action is not automatically waiting for a model;
use an actual provider request observation when available. A missing action
completion means completion was not observed, not success or a proven hung
worker. Backend freshness/errors are separate from action outcomes. During a
backend outage the UI retains bounded last-known data, marks it stale, and
refreshes it on recovery. Trace links inspect standard backend data, not the
Evidence Store or local provider logs.

Telemetry is best effort with bounded export/query/history limits. A hard
process loss may leave starts without terminals. Existing durable cycle state
still governs scheduling and recovery; telemetry does not change Candidate
admission, activation or runtime authority. Live authenticated Copilot tests
are optional; deterministic integration tests cover the full action path.

## Durable cycle behavior

The physical workspace directory is the SHA-256 digest of the contract name;
the manifest retains the original name. Completed cycles are append-only.
Recoverable provider, tool, HTTP, and model failures keep the current cycle
open so a fresh agent session can resume from durable artifacts.

Candidate proposal is journaled before submission. A retry reuses the original
attempt identity and exact rules, allowing safe recovery if the process loses
the HTTP response. If activation or rejection wins before that retry, Contract
Service returns the original admission with its current lifecycle and
`propose_executable` records the submission as successful without reopening it.
A different second proposal is rejected. Analysis artifacts are sealed into a
content-digested manifest before admission; mutable host control records and
Copilot session state are excluded. The manifest records the exact ordered
analysis skill names used by the cycle, and resume under a different skill set
or order is rejected.

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
`describe` publishes the normalized decision-attribute and metric
data-point JSON paths. Finite OTel doubles retain their exact bit encoding and
also expose a decimal `value` for SQL analysis. Metric observations may be
cumulative snapshots, so analysis must use the latest point or changes between
points for each correlated population rather than summing repeated exports.

`run_sql` does not accept arbitrary SQLite. It accepts one validated,
read-only `SELECT` or non-recursive `WITH` statement over the logical
`observations` relation and named CTEs. Grouping, ordering, arithmetic, `CASE`,
JSON extraction, and common aggregates are supported. The current approved
functions are `abs`, `avg`, `coalesce`, `count`, `json_extract`, `json_type`,
`lower`, `max`, `min`, `nullif`, `round`, `sum`, `total`, and `upper`;
`describe` reports this list to the agent. Authority, declared source, and
exact contract digest are always injected by trusted code. Time-window
predicates are chosen by the agent. Multiple statements, recursive CTEs,
mutation, pragmas, physical tables, table-valued functions, and unapproved
functions are rejected, and results are bounded by row, byte, and timeout
limits. Percentiles, median, standard deviation, regression, and richer
statistical operations are not available yet.

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
