# Analysis workspace contracts and local adapter

The portable `WorkspaceProvider`, `WorkspaceLease` and virtual filesystem
contracts describe cycle preparation, handoffs, budgets, candidate sealing,
recovery and relative artifact access. They do not require a host directory.
Use `default-features = false` to consume these contracts without the local
filesystem implementation, temporary-file dependency or local UUID allocation.

The default `local` feature supplies `LocalWorkspaceProvider`. Local Python
composition receives that provider separately; only its
`analysis_directory(&lease, cycle_id)` convenience resolves a host path. It
requires the exact live claim allocation from that provider, an active cycle
and writable/unsealed state. An equal workspace name/ID from another provider
does not grant access. Weak claim references do not retain ownership after a
lease is dropped.

File locks, atomic local publication, handoff-pointer recovery, proposal
sealing and existing budget behavior are retained. The local process remains
trusted and unsandboxed: resolving a local path is not a remote execution
protocol or isolation mechanism. Portable `flaggo.workspace/v2` claims bind
full resource identity and monotonic epochs. Immutable artifact handles,
conditional handoff publication and `flaggo.execution/v1` reservations/receipts
support recovery without replaying uncertain side effects.

The local provider also implements `AdmissionFence`. Its OS mutation gate
coordinates claim/cycle transitions with the Contract Service SQLite admission
commit. Both apps must share `FLAGGO_ANALYSIS_WORKSPACE_ROOT`. This local proof
does not qualify distributed lease expiry, cross-host storage or hosted execution.

```powershell
cargo test -p flaggo-analysis-workspace --locked
cargo check -p flaggo-analysis-workspace --no-default-features --locked
cargo test -p flaggo-async-analysis --locked
```

Analysis tests require the checkout-local Python environment documented in
[`examples/README.md`](../../examples/README.md#python-environment-for-live-analysis).
