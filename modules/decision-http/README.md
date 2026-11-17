# Shared Decision HTTP application

`flaggo-decision-http` owns v3 routing, input/error handling, telemetry and health
responses. `router` and `instrumented_router` accept implementations of existing
`ContractVersionReader` / `ActiveExecutableReader` plus `ReadinessProbe`.
The shared `DecisionRuntime` remains the only evaluator orchestrator.

There is no SQLx, SQLite or runnable-app dependency. The local Decision Service
supplies SQLite readers; another composition supplies its own authoritative
readers and probes. A probe checks required schema/capabilities, not every artifact.
Active authority is read on every decision; compiled immutable artifacts may be
cached, but a failed active read cannot serve an old cached activation.

```powershell
cargo test -p flaggo-decision-http --locked
cargo test -p flaggo-decision-service --locked
```

The portable test uses injected non-SQLite readers. It establishes local HTTP
behavior and cache/error handling, not multi-host linearizability.
