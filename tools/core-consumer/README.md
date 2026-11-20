# Separate immutable-revision consumer

Run `check.py` with an exact committed product revision, a Git repository URL and
a **new** output directory outside this workspace's Cargo package tree. It builds
an independent client, retains `Cargo.lock`, runs canonical digest/evaluation
checks (including the reusable C01 corpus and C02 exact-evaluation parity),
executes shared bootstrap/admission/activation/resolution and persisted-contract/executable
integrity policy without SQLite,
executes the database-free evidence projection policy corpus and default
single-resource/batch admission, composes retention-independent private OTLP health,
compiles all three HTTP compositions and shared materialization/analysis/execution
capabilities, and rejects SQLite, app composition roots or unpinned product
dependencies. It never imports runnable apps or copies product implementation.

```powershell
python tools\core-consumer\check.py --repository https://github.com/hahahahahaiyiwen/flaggo.git --revision <full-commit> --output C:\temp\flaggo-consumer
```

The revision must be reachable at that repository. For unpublished local work,
an explicit `file:///C:/...` Git URL can test the same consumption mechanism.
The generated result labels that **local-git-consumer**, not public remote
publication or hosted acceptance. Keep the generated manifest, lock and result
with qualification evidence. Rerun a preserved consumer with `cargo run --locked`;
do not regenerate its lock to claim the same dependency qualification.
