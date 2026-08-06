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
  registration, and writes receipt-bound state.
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
  --state .flaggo\tetris-state-v1.json `
  --receipt .flaggo\tetris-registration-receipt.json
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
Ports are reserved together to guarantee distinct control, data, and
unavailable endpoints. Child hosts and log streams are closed and generated
files are removed on both success and failure.

The `Contracts` GitHub Actions workflow runs this command in a dedicated
`tetris-integration` job with Node 20 and .NET 10. The job uses only local
processes and files; it requires no cloud service or secret and is kept
separate from `npm test`.
