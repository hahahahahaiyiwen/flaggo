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
protocol, isolation mechanism or durable distributed fence. Portable
revision/fence/artifact receipts and hosted execution are still separate #94
and later-phase work.

```powershell
cargo test -p flaggo-analysis-workspace --locked
cargo check -p flaggo-analysis-workspace --no-default-features --locked
cargo test -p flaggo-async-analysis --locked
```

Analysis tests require the checkout-local Python environment documented in
[`examples/README.md`](../../examples/README.md#python-environment-for-live-analysis).
