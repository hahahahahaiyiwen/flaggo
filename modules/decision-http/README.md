# Shared Decision HTTP application

`flaggo-decision-http` owns scoped v4 routing, input/error handling, telemetry and health
responses. `router` and `instrumented_router` accept implementations of existing
`ContractVersionReader` / `ActiveExecutableReader` plus `ReadinessProbe` and an
independently injected `RequestSecurity`.
The shared `DecisionRuntime` remains the only evaluator orchestrator.

The shared [authorized request-admission hook](../hosting/README.md#authorized-request-admission)
runs after resource/action authorization and before runtime input validation or
authority lookup. A metered adapter may return 429 before a later handler error;
accepted attempts are not refunded on failure or cancellation, and retries are
new attempts. The default is non-metered and health routes bypass the hook.

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
