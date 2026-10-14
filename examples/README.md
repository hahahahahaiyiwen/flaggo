# Examples

The current examples use v3 `DecisionContract` deployment, exact-version
runtime calls, and application-owned OpenTelemetry.

Examples demonstrate Flaggo integrations without becoming production module
dependencies.

## Python environment for live analysis

Before running an example with live Copilot analysis, such as the Tetris
playground or the terminal game's opt-in live-agent extension, install Python
3.12 or newer and prepare a virtual environment **from the repository root,
not from `examples`**.

On Windows (PowerShell):

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r .\apps\async-analysis\requirements.txt
.\.venv\Scripts\python.exe -m pip check
.\.venv\Scripts\python.exe -c "import numpy, pandas, scipy, sklearn, matplotlib, seaborn; print('Analysis packages ready')"
```

On Linux/macOS:

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r apps/async-analysis/requirements.txt
.venv/bin/python -m pip check
.venv/bin/python -c "import numpy, pandas, scipy, sklearn, matplotlib, seaborn; print('Analysis packages ready')"
```

The tracked [analysis requirements](../apps/async-analysis/requirements.txt)
provide NumPy, pandas, SciPy, scikit-learn, matplotlib and seaborn. Versions are
not pinned, so this setup recreates the package set, not an identical locked
environment. The generated `.venv` is ignored by Git; create it separately in
each checkout/worktree rather than copying or committing it.

Async Analysis defaults to this checkout's `.venv` interpreter without
activation or setting `FLAGGO_ANALYSIS_PYTHON`. An explicit
`FLAGGO_ANALYSIS_PYTHON` overrides that default; there is no fallback to system
Python. The default location is bound to the source checkout when Rust is
built, so rebuild after moving the checkout. Python runs unsandboxed under
your local account; use a trusted development machine. See the
[Async Analysis guide](../apps/async-analysis/README.md) for execution limits
and configuration.

Continue with the chosen example's local-run instructions after setup.
Standalone terminal play and deterministic benchmarks do not require Python
analysis.

## Available examples

Small, self-contained examples may live here. Larger showcase applications,
including the Tetris frontend, may remain external repositories and pin a
released SDK and exact deployed contract version. Every example must document a cloud-free local
run path.

`tetris-integration` is a playable terminal Tetris app. It runs independently
with local level-based gravity, or optionally starts local Contract and
Decision Services and delegates the drop interval through the TypeScript SDK.
Its Flaggo mode shares the playground's adaptive contract and session-owned
experience tracker/provider. The automated harness runs genuine greedy
episodes, verifies exposure-correlated four-gauge evidence, SDK/REST parity,
restart persistence, and retired-field absence. Its opt-in live-agent extension
uses the same nine-game training protocol and verifies Candidate auto-activation.

`tetris-playground` composes that headless game boundary with all real Flaggo
services, a pinned Collector and Grafana LGTM, and a browser observer. Its
default live mode uses credentialed production Copilot analysis, one dominant
human game, and ten initially idle real-time greedy slots contributing
attributed evidence. The separate `--mode=benchmark` launch defaults to the
deterministic fitter and measures matched held-out improvement under the
actually activated executable. Live learning does not guarantee a Candidate
or improved human enjoyment. See
[the playground guide](tetris-playground/README.md).

`adaptive-worker` deploys one contract and processes a deterministic
in-memory queue through the exact-version Decision Service. It covers both
default and authored-rule decisions and records native application OTel logs
without cloud services.
