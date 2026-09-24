# Native OTel evidence

This cloud-free example runs the real application OTel SDKs, stock Collector,
Flaggo hosts, and manifest-first client. No Flaggo producer, telemetry envelope,
or SDK-managed exporter is involved.

## Run

Requirements: Node.js 20+, .NET 10, repository npm/NuGet dependencies, and the
stock **OpenTelemetry Collector core 0.161.0** executable. The harness checks
the exact version and validates the complete `collector.yaml` before starting
it. It deliberately uses `otlp_http`, not the deprecated `otlphttp` alias.

From the repository root, restore missing dependencies:

```powershell
npm ci
dotnet restore Flaggo.slnx --source https://api.nuget.org/v3/index.json
```

Use an existing verified 0.161.0 executable, or download the official Windows
AMD64 release into a repository-local tool directory:

```powershell
$toolDir = Join-Path (Get-Location) '.flaggo\tools\otelcol-0.161.0'
New-Item -ItemType Directory -Path $toolDir -Force | Out-Null
$archive = Join-Path $toolDir 'otelcol_0.161.0_windows_amd64.tar.gz'
Invoke-WebRequest -Uri 'https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.161.0/otelcol_0.161.0_windows_amd64.tar.gz' -OutFile $archive
$expected = 'D51435B421F78BCBB17200ADC2EE802ABF629BC77DCB2EDD44C2566078515F08'
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
  throw 'Collector archive checksum mismatch.'
}
tar -xzf $archive -C $toolDir
if ($LASTEXITCODE -ne 0) { throw 'Collector extraction failed.' }
$env:FLAGGO_OTELCOL_PATH = Join-Path $toolDir 'otelcol.exe'
npm run test:otel-evidence
```

The digest is from the release's
[`windows_amd64.tar.gz.sha256`](https://github.com/open-telemetry/opentelemetry-collector-releases/releases/download/v0.161.0/otelcol_0.161.0_windows_amd64.tar.gz.sha256).
The CI job downloads the same version's Linux AMD64 archive and verifies its
pinned digest. No global Collector install, cloud account, or real credential
is needed.

The command compiles the one `decision-manifest.json` into a normalized bundle
and typed catalog, builds the hosts, explicitly publishes/approves the contract
through trusted local management tooling, and provisions a receipt-bound local
numeric rule. This is not automatic approval by the runtime SDK or #40's
future manifest initial-authority workflow.

## What it demonstrates

| Existing application observation | Manifest-owned operand | Value |
| --- | --- | --- |
| Gauge `work.queue.pressure` | `pressure` | `0.75` |
| Span `work.process`, duration | `durationMs` | `250` ms |
| Span event `work.retry`, attribute | `retries` | `2` |
| Structured log `work.summary`, body field | `failures` | `2` |

`telemetry.mjs` owns ordinary OTel providers and OTLP exporters. The Collector
fans out each signal to both `debug/existing` and the additional scoped Flaggo
exporter. The harness verifies the existing branch still receives all three
signal kinds. Replace that demonstration debug branch with the application's
existing backend without changing the Flaggo client.

The client supplies a key and session context, not evidence values:

```js
const decision = await client.tune.numberDetailed("worker.batchSize", {
  context: { sessionId: "worker-session" },
  idempotencyKey: "collector-evidence"
});
```

Before observations arrive, required input evidence fails closed and does not
trigger SDK fallback. After durable materialization, the rule returns batch
size **6**, with `confidence: null`. The persisted audit contains the exact
four resolved values, empty caller inputs, one pinned generation, source
nanosecond times, and trace/span correlation. Retrying the successful decision
returns the retained outcome rather than resolving newer telemetry.

The application applies the batch size, explicitly confirms the exposure,
then attaches `confirmedExposureAttributes` to an ordinary `work.completed`
log. A fifth manifest binding verifies the completed confirmation, exact
definition, authorized scope, and session target before durably recording the
processed count `6`. Merely returning a decision creates no exposure.

## Operational boundaries

- Receivers bind only to loopback; ports and paths are isolated per run.
  Failures print bounded host diagnostics, and cleanup stops only owned
  children and removes only that run's directory. Downloaded tools are retained.
- Collector routing, credentials, buffering and sampling are external to the
  decision manifest and runtime client. `Flaggo-Local-Telemetry` is a distinct,
  Development-only ingest credential; do not use it outside local examples.
- Freshness is 60 seconds from source observation time, not arrival time.
  All evidence is explicitly **observed-only**. The test's AlwaysOn trace
  sampler does not grant population completeness or learned confidence.
- SDK-dropped spans cannot be recovered by Collector fan-out. Tail sampling,
  log filtering, and unknown inclusion can bias received data. Gauge snapshots
  are not counter rates; exemplars cannot attribute an entire metric series
  to one exposure.
- Only latest scalar Gauge, selected span/span-event, and structured log
  projections are supported. No histogram conversion, rolling aggregation,
  arbitrary joins, JSON-text parsing, or sampling correction is implied.
- Confirmation storage is currently in-memory. Verified durable input frames
  survive restart; newly arriving references to lost confirmations fail closed.

The [Tetris](../tetris-integration/README.md) and
[worker](../adaptive-worker/README.md) examples keep genuinely current local
state as request inputs instead of waiting for telemetry export.
