# Adaptive Worker Example

This example owns the Phase 2.5 local SDK-to-service acceptance boundary. It
is a TypeScript console application that processes real items from an
in-memory queue while Flaggo decides `demo.workerBatchSize`.

## Design

- `decision-manifest.json` is the sole authored contract. Compilation generates
  `generated/definitions.json` and `src/generated/catalog.ts`.
- Trusted service bootstrap publishes/approves the bundle separately. The
  worker initializes synchronously from its catalog and the approved receipt.
- The local service bootstrap publishes the accepted receipt, governed state,
  and an empty policy-evidence artifact as one digest-pinned generation.
  Inference uses plain live inputs and no evidence provider or learned confidence.
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
- Application-owned OTel logging includes `worker.item.enqueued`,
  `worker.item.completed`, `worker.queue.depth`, `worker.queue.pressure`,
  `worker.processing.latency`, and `worker.batch.applied`. The last record
  attaches the explicit confirmation attributes. The example's OTel provider
  uses an in-memory exporter and writes captured native records locally;
  Flaggo does not install an exporter or define those log schemas.

The example does not feed telemetry back into evidence, derive metrics on the
server, persist the queue, or require the Tetris application.

## Automated acceptance

From the repository root:

```powershell
npm run test:adaptive-worker
```

The smoke test builds the SDK and application, regenerates and verifies the
manifest artifacts, starts isolated control- and data-plane hosts, runs the
steady, burst, slow-downstream, and recovery profiles, verifies fallback and
exposure behavior, and removes its temporary local state.

## Manual local run

Build the SDK, manifest artifacts, application, and .NET hosts:

```powershell
npm run build:adaptive-worker
dotnet build Flaggo.slnx -c Debug --no-restore
```

Start the local service scenario:

```powershell
node examples\adaptive-worker\service.mjs
```

The service prints the control- and data-plane URLs and writes the active
connection details below `.flaggo/adaptive-worker/service.json`. In another
terminal, run the worker:

```powershell
node examples\adaptive-worker\dist\main.js
```

Stop the service with `Ctrl+C`. The launcher also exits and cleans up if either
managed .NET host terminates unexpectedly. The worker uses server decisions by default;
pass `--allow-local-fallback` only when explicitly exercising SDK availability
fallback.

The manual launcher exclusively creates `.flaggo/adaptive-worker`; an existing
directory is not deleted or taken over. Inspect a stale prior run before
removing its specific resources. Normal shutdown removes only this owned
example directory. See [the Collector example](../otel-evidence/README.md) for
native telemetry ingestion instead of local capture.

## Maintenance

Keep this README, the generated artifact test, the smoke assertions, and the
service bootstrap aligned whenever the worker contract, queue-pressure
formula, policy, target mapping, application instrumentation, or fallback behavior
changes. Shared wire changes must update OpenAPI/schema fixtures and both SDK
and service conformance suites before this example diverges.
