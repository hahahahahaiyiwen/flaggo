# Adaptive Worker Example

This example owns the Phase 2.5 local SDK-to-service acceptance boundary. It
is a TypeScript console application that processes real items from an
in-memory queue while Flaggo decides `demo.workerBatchSize`.

## Design

- Static extraction owns the canonical definition bundle and decision
  descriptor under `generated/definitions.json`.
- Startup registration must complete before the worker creates runtime
  decisions.
- The local service bootstrap publishes the accepted receipt, governed state,
  and deterministic evidence as one digest-pinned generation.
- The service authoritatively maps the claimed `worker-canary` cohort to the
  governed `adaptive-workers` target.
- Queue pressure is derived only from queue depth and the oldest queued item:

  ```text
  clamp(
    0.7 * queueDepth / queueCapacity
    + 0.3 * oldestItemAgeMs / targetLatencyMs,
    0,
    1
  )
  ```

- The numeric action space is `1..10`, step `1`, with fallback `3`.
- The deterministic rule returns `3` below pressure `0.7` and `6` at or above
  `0.7`. Policy enforces bounds, maximum delta `3`, and a short cooldown.
- A returned batch size is applied before exposure confirmation. Repeated
  confirmation must preserve the original exposure identity. A lost
  confirmation response remains pending and is retried idempotently before
  another decision can be processed.
- SDK availability fallback is disabled unless the caller explicitly enables
  local fallback.
- Telemetry remains local and includes `demo.itemEnqueued`,
  `demo.itemCompleted`, `demo.queueDepth`, `demo.queuePressure`, and
  `demo.processingLatencyMs`.

The example does not feed telemetry back into evidence, derive metrics on the
server, persist the queue, or require the Tetris application.

## Automated acceptance

From the repository root:

```powershell
npm run test:adaptive-worker
```

The smoke test builds the SDK and application, regenerates and verifies the
static artifact, starts isolated control- and data-plane hosts, runs the
steady, burst, slow-downstream, and recovery profiles, verifies fallback and
exposure behavior, and removes its temporary local state.

## Manual local run

Build the SDK, extracted artifact, application, and .NET hosts:

```powershell
npm run build:adaptive-worker
dotnet build Flaggo.slnx -c Debug --no-restore
```

Start the local service scenario:

```powershell
node examples/adaptive-worker/service.mjs
```

The service prints the control- and data-plane URLs and writes the active
connection details below `.flaggo/adaptive-worker/service.json`. In another
terminal, run the worker:

```powershell
node examples/adaptive-worker/dist/main.js
```

Stop the service with `Ctrl+C`. The launcher also exits and cleans up if either
managed .NET host terminates unexpectedly. The worker uses server decisions by default;
pass `--allow-local-fallback` only when explicitly exercising SDK availability
fallback.

## Maintenance

Keep this README, the extracted artifact test, the smoke assertions, and the
service bootstrap aligned whenever the worker contract, queue-pressure
formula, policy, target mapping, telemetry schema, or fallback behavior
changes. Shared wire changes must update OpenAPI/schema fixtures and both SDK
and service conformance suites before this example diverges.
