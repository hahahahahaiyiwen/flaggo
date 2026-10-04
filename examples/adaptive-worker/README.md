# Adaptive Worker Example

This TypeScript console application processes an in-memory queue while Flaggo
chooses `demo.workerBatchSize`.

`flaggo/contracts/demo.workerBatchSize.decision-contract.json` is the authored,
authority-free decision definition, and `flaggo.deploy.json` lists it for
deployment. The local environment:

1. starts Contract Service and Decision Service against one isolated SQLite
   database;
2. starts Flaggo OTel Ingestion against the same database;
3. injects the manifest authority and deploys the resulting complete contract
   through
   `PUT /v3/decision-contracts/demo.workerBatchSize`;
4. generates one immutable `flaggo.runtime-config/v1` document containing the
   manifest authority, all three service URLs, and the returned exact
   authority-bound `contractDigest`; and
5. gives that configuration to both the decision client and telemetry
   providers.

The authored executable returns `6` when `queuePressure >= 0.7`; otherwise the
contract default is `3`. The worker supplies `workerId`, `cohort`, and
`queuePressure` as ordinary decision attributes. The SDK adds `_random`.
Targets, approval receipts, runtime idempotency keys, server decision records,
confirmation calls, generic constraints, and SDK fallback are not part of the
v3 flow.

Application-owned OpenTelemetry providers export logs, metrics, and traces
directly to Flaggo OTel Ingestion. `worker.item.enqueued`,
`worker.item.completed`, `worker.batch.applied`, and
`worker.processing_latency` are structured application logs. The processing
latency log body is the exact measured value, and its attributes carry the
contract-declared worker correlation values. Queue depth, queue pressure,
selected batch size, and processing latency are also metrics. Each
`worker.tick` is a span. The SDK emits the built-in
`flaggo.decision.received` observation. All logs correlate with the active
worker span. OTLP success acknowledges durable inbox enqueue rather than
immediate Evidence Store materialization. Runtime evaluation itself remains
stateless. The same generated authority
(`local / adaptive-worker / development`) covered by the contract digest is
attached to the shared OTel Resource as `flaggo.tenant`,
`flaggo.application`, and `flaggo.environment`. Ordinary application logs,
metrics, and spans remain contract-agnostic; exact contract provenance belongs
to the SDK-owned decision observation.

## Automated acceptance

From the repository root:

```powershell
npm run test:adaptive-worker
```

The smoke test builds the SDK, application, .NET decision hosts, and Rust
ingestion host; deploys the contract; exercises default and authored-rule
decisions through the real Decision Service; verifies direct OTLP/HTTP JSON
requests for all three signals against the real ingestion host; checks that an
unavailable service is surfaced as a failure; and removes its isolated state.

## Manual local run

```powershell
npm run build:adaptive-worker
npm run build:rust
dotnet build Flaggo.slnx -c Debug --no-restore
node examples\adaptive-worker\service.mjs
```

The launcher writes the generated immutable runtime configuration to
`.flaggo/adaptive-worker/flaggo.runtime.json`. In another terminal:

```powershell
node examples\adaptive-worker\dist\main.js
```

The launcher supplies the dynamically assigned service URLs, deployment
authority, and immutable contract binding through that configuration. The
application owns all three OpenTelemetry providers and their shutdown, while
`@flaggo/sdk/opentelemetry` supplies only the additional Flaggo processors and
metric reader. Stop the launcher with `Ctrl+C`; it owns and removes only its
specific `.flaggo/adaptive-worker` directory.
