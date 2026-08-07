# Tetris Phase 3 Local Integration

This example is the trusted, backend-only integration boundary for
`tetris.dropInterval`. It does not contain frontend game behavior.

## Artifacts

- `tetris-definition-bundle.json`: canonical SDK and management bundle.
- `strategy-activation.json`: application-neutral governed numeric-rule
  configuration using all four live inputs.
- `evidence.json`: deterministic local confidence evidence for the activated
  strategy. The canonical policy requires at least `0.7` evidence quality and
  forbids client fallback when the active strategy entry is unavailable.
- `bootstrap.mjs`: applies the bundle with the real SDK, handles typed
  `requires-approval`, approves through the management API, retries
  registration, and atomically publishes receipt-bound state, evidence, and
  receipt as one generation.
- `run.mjs`: deterministic real-host integration harness.
- `inspect.mjs`: local audit and linked outcome summary.

The generated state identity always comes from
`receipt.acceptedDefinitions["tetris.dropInterval"]`; it is never duplicated in
the activation fixture. The canonical bundle contains no approval hint:
bootstrap approval behavior is triggered only by the control plane's typed
`requires-approval` response.

## Trusted bootstrap

Build the SDK and hosts, start the control plane with local-development
authentication and an isolated registry path, then run:

```powershell
node examples\tetris-integration\bootstrap.mjs `
  --control-plane http://127.0.0.1:5081 `
  --data-plane http://127.0.0.1:5080 `
  --output .flaggo\tetris-bootstrap
```

The command is trusted startup tooling. Do not move it or management
credentials into browser code.

## Outcome linkage contract

After applying a server receipt:

1. call `flaggo.exposures.confirm(decisionId, confirmToken)`;
2. emit `tetris.outcomeObserved` through the configured SDK `TelemetrySink`;
3. set `decisionId` and `exposureId` from the confirmed result;
4. include the applied `dropIntervalMs` and application outcome.

Client fallback and unused receipts have no confirmed exposure and must not
emit this linked event.

## Automated run

```powershell
npm run test:tetris-integration
```

The harness uses repository-local `.flaggo/integration-*` paths, starts
separate control/data processes, and removes its generated files afterward.
It also replaces the valid evidence document with one lacking the active
strategy and verifies that the SDK receives fail-closed
`required-evidence-unavailable` rather than a wire-invalid strategy result.
That `503` uses an idempotency key; after evidence is restored, the harness
retries the same key and verifies a fresh successful governed decision.
Each ASP.NET host binds directly to loopback port `0`; the harness enables
structured JSON console logs and discovers the assigned listening URL before
making requests. This removes the allocate-close-bind race, including the
data-plane process started after bootstrap. The SDK fallback check uses a
dedicated loopback server that returns a deterministic fallback-eligible `503`,
binds port `0`, and remains bound until the check completes, so it cannot race
another process for its endpoint.
Every readiness fetch receives a per-probe abort signal bounded by the
remaining overall readiness deadline and combined with lifecycle cancellation;
an accepted connection that never responds therefore cannot extend startup.
Readiness timeout errors retain the latest HTTP status and response body so
dependency states such as unavailable audit storage are visible without
reconstructing them from host logs. A later transport failure or probe deadline
is reported alongside that meaningful diagnostic instead of replacing it.
Both `ASPNETCORE_ENVIRONMENT` and `DOTNET_ENVIRONMENT` are forced to
`Development`, regardless of parent-process values. `SIGHUP`, `SIGINT`, and
`SIGTERM` abort the active bootstrap/workflow before another host can start.
Cleanup first closes host registration, drains every in-flight factory and
host stop, then removes the run directory. A factory that resolves after
cleanup begins is immediately stopped and fully drained rather than becoming
active. Transient Windows-style open-log removal races are retried after stop
completion. Cleanup failures are aggregated and reported without replacing the
workflow failure; repeated signals and cleanup calls are idempotent. Any host
exit before its requested stop, including exit code zero, is checked at startup
boundaries and again during cleanup and fails an otherwise passing run.

Bootstrap writes receipt, state, and evidence through explicit handles into a
unique `generations/<id>` directory. All sibling writes are settled, every file
and the generation directory are flushed, and only then is `current.json`
atomically replaced and its parent directory synced where supported.
Directory-open permission failures are surfaced. Platform-specific directory
flush results that mean the runtime/filesystem does not support directory sync
remain explicit best effort.
Pre-switch failures remove the unpublished generation; old generations remain
available to readers that already resolved them. The data-plane composition
root resolves the pointer once and obtains state and evidence from that same
generation, while the receipt remains a bootstrap/SDK output rather than a
runtime adapter input. Audit inspection follows the durable audit manifest.

The `Contracts` GitHub Actions workflow runs this command in a dedicated
`tetris-integration` job with Node 20 and .NET 10. The job uses only local
processes and files; it requires no cloud service or secret and is kept
separate from `npm test`.
