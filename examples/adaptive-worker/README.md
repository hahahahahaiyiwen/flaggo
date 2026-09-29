# Adaptive Worker Example

This TypeScript console application processes an in-memory queue while Flaggo
chooses `demo.workerBatchSize`.

`flaggo/contracts/demo.workerBatchSize.decision-contract.json` is the complete
v3 `DecisionContract`, and `flaggo.deploy.json` lists it for deployment. The
local environment:

1. starts Contract Service and Decision Service against one isolated SQLite
   database;
2. deploys the contract through
   `PUT /v3/decision-contracts/demo.workerBatchSize`;
3. records the returned exact `contractDigest`; and
4. configures the worker to call the exact-version Runtime API.

The authored executable returns `6` when `queuePressure >= 0.7`; otherwise the
contract default is `3`. The worker supplies `workerId`, `cohort`, and
`queuePressure` as ordinary decision attributes. The SDK adds `_random`.
Targets, approval receipts, runtime idempotency keys, server decision records,
confirmation calls, generic constraints, and SDK fallback are not part of the
v3 flow.

Application-owned OpenTelemetry logging includes
`worker.item.enqueued`, `worker.item.completed`, `worker.queue.depth`,
`worker.queue.pressure`, `worker.processing.latency`, and
`worker.batch.applied`. The SDK also emits `flaggo.decision.received`, and the
example emits `flaggo.outcome.observed` for
`demo.workerBatchSize.processingLatencyMs`. Runtime evaluation itself remains
stateless.

## Automated acceptance

From the repository root:

```powershell
npm run test:adaptive-worker
```

The smoke test builds the SDK, application, and .NET hosts; deploys the
contract; exercises default and authored-rule decisions through the real
Decision Service; verifies application telemetry; checks that an unavailable
service is surfaced as a failure; and removes its isolated state.

## Manual local run

```powershell
npm run build:adaptive-worker
dotnet build Flaggo.slnx -c Debug --no-restore
node examples\adaptive-worker\service.mjs
```

The launcher writes connection details below
`.flaggo/adaptive-worker/service.json`. In another terminal:

```powershell
node examples\adaptive-worker\dist\main.js
```

Stop the launcher with `Ctrl+C`. It owns and removes only its specific
`.flaggo/adaptive-worker` directory.

To stream example logs to a local OpenTelemetry Collector, start the Collector
with `deploy\otel-collector-flaggo-local.yaml` and set:

```powershell
$env:FLAGGO_OTEL_COLLECTOR_LOGS_URL = "http://localhost:4318/v1/logs"
node examples\adaptive-worker\dist\main.js
```

The Collector forwards OTLP logs to `Flaggo.OtelIngestion` at `/v1/logs`; the
ingestion service performs Flaggo signal filtering and writes accepted raw OTLP
log records to Evidence Store.
