# Phase 5 exit: reviewed analysis on Rust services

Issue [#74](https://github.com/hahahahahaiyiwen/flaggo/issues/74) integrates
the delivered SQL/Python, proposer/reviewer, action observability, BYOK and
Rust-server work. This is an executable acceptance map, not a stronger
production guarantee or a new roadmap.

## Deterministic exit scenario

`npm run test:tetris-playground:e2e` starts the existing real-service benchmark
and opts the example-owned deterministic provider into
`FLAGGO_TETRIS_ANALYSIS_SCENARIO=reviewed-python-exit`.
Ordinary playground launches and the terminal full-loop test retain their
existing deterministic behavior. The production Copilot provider is unchanged.
The four-attempt exit scenario has a three-minute activation observation
window; the ordinary deterministic demo retains its 45-second window.

The exit scenario reads paginated, scoped SQL into saved query files. A fresh
Python process combines them, checks duplicate observation identities, orders
the evidence and computes signal coverage. The resulting saved dataset is the
input to the existing deterministic contextual fitter; this does not introduce
a second statistical model or claim Python confinement.

The first proposer deliberately writes an unsupported constant-policy draft.
A separate reviewer reproduces the saved evidence, retains its findings and
requests a change. The next proposer reuses the same dataset and execution
record without repeating queries or Python. A fourth attempt reviews the
corrected draft and submits it through normal Candidate admission. Contract
Service then activates it; SDK and direct REST decisions must agree on the new
digest and contextual behavior.

The test checks the actual workspace artifacts against the sealed manifest
and Candidate provenance. It checks two distinct drafts, both reviews,
handoff identities, saved query metadata/digests, one Python execution and its
submitted code, and profile/action correlation through Loki. Artifact access
is test-only: the browser and gateway still do not read analysis workspaces.
The benchmark's synthetic improvement is not evidence of live-model quality,
causal validity for arbitrary workloads, or universal guardrail satisfaction.
All services must export their supported logs, operation spans and metrics.
Successful Rust Decision Service requests emit spans/metrics, not per-request
logs; its lifecycle logs need not have an active trace. The exit does not add
a stronger per-decision logging requirement merely to satisfy the old test.

## Acceptance map

| Requirement | Executable evidence |
| --- | --- |
| SQL-to-file, Python, unsupported draft, review, correction, activation and SDK/REST parity | `examples/tetris-playground/test/closed-loop.spec.mjs`, real Rust hosts and SQLite, Collector/LGTM and Chromium |
| Newly materialized old events visible only to a new query; old work survives restart without replay | `real_sqlite_to_python_artifacts_survive_workspace_restart_without_replay` in `apps/async-analysis/src/local_capabilities.rs`; live-query tests in `modules/evidence-store/tests/sqlite_analysis.rs` |
| Complete/truncated/empty query metadata, unknown totals, scoped SQL | `local_capabilities` tests and Evidence Store analysis tests |
| Python failures, launch failure, timeout/cancellation, bounded capture and fresh memory | `apps/async-analysis/src/python.rs` and `local_capabilities` tests |
| Permissions, immutable referenced drafts/datasets, revision/rejection and durable budgets | `modules/analysis-agent/src/workflow.rs`; `apps/async-analysis/tests/analysis_pipeline.rs` |
| Identical sealed retries/lost replies, conflicting submissions and cached currentness | `analysis_pipeline.rs`, workspace tests and real Contract Service admission tests |
| Prior runtime authority retained on failed/superseded work or failed activation | Coordinator and Contract Service tests; failed activation and SDK/REST assertions in the browser loop |
| Bounded, partial, duplicated, reordered or stale action observations; stable inspection focus | `analysis-actions.test.mjs`, `browser-smoke.spec.mjs`, and real OTLP action correlation in `closed-loop.spec.mjs` |
| Rust contracts, expressions, identity, stores and activation | Workspace Rust suites, contract conformance, terminal full-loop and adaptive-worker gates |
| Explicit BYOK/auth/error configuration without fallback | Async Analysis configuration, Copilot adapter and launcher tests; recorded credentialed evidence below |

CI runs the workspace Rust checks and required deterministic terminal/browser
gates without provider credentials. The browser gate needs Docker Compose and
Chromium. Install JavaScript dependencies with `npm ci`; prepare the checkout's
Python environment using the [Async Analysis setup](../../apps/async-analysis/README.md#run-locally).
Install the browser with `npx playwright install chromium` (CI also installs
system dependencies).

Useful focused commands:

```powershell
cargo test -p flaggo-async-analysis -p flaggo-analysis-agent -p flaggo-analysis-workspace -p flaggo-evidence-store -p flaggo-tetris-playground-analysis --locked
npm run test:tetris-playground
npm run test:tetris-playground:browser
npm run test:tetris-playground:e2e
```

Failures retain bounded diagnostics and Playwright artifacts in
`examples/tetris-playground/test-results`;
fixture cleanup stops its services/containers and removes its run directory.
Missing Python, Node, Docker or Chromium is a setup failure, not a successful
or skipped exit. A missing completion event is not success.

## BYOK evidence and accepted limitation

On 2026-11-09 the owner accepted the documented credentialed run from
[#81](https://github.com/hahahahahaiyiwen/flaggo/issues/81) as sufficient
BYOK evidence for this exit, retaining its limitations. The run used
`gpt-5.4-nano-global` (`gpt-5.4-nano`, version `2026-03-17`), Responses,
Rust SDK `1.0.17-preview.5`, runtime `1.0.92-5` / protocol `3`, with GitHub
token variables removed and process-only endpoint credentials.

After a response-schema fix, real iterative SQL and saved-artifact reads
worked. The ten-minute observation ended without a terminal cycle outcome.
Hosted completed review, Candidate generation and end-to-end live success
remain **unverified**. This acceptance does not turn that timeout into a pass,
promise arbitrary provider/model compatibility, or require another
credentialed run. Credentials and private endpoint details are not part of
the evidence map.

## Guarantee boundaries

Queries are live, not a frozen cycle-wide snapshot. Query times are not
completeness watermarks. Saved datasets and execution records are durable
work products, not complete I/O provenance or tamper-proof host artifacts.
Python is deliberately trusted-host/unsandboxed; direct-process termination
does not contain descendants or enforce CPU/memory/artifact quotas.

Provider-owned transitions and protocol review remain separate from generic
coordinator lifecycle/budgets. This scenario adds no trusted review-approval
gate. At most one Candidate admission permits identical sealed retries.
Activation is owned by Contract Service; that supported workflow boundary is
not a claim that host Python cannot contact services or alter host resources.
Packaging, public hosting and outreach remain Phase 6 work.
