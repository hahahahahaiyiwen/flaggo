# Internal service observability

## Status and authority

This document defines operational observability for Flaggo-owned services and
workers. The machine-readable authority is
[`flaggo-service-observability-profile-v1.json`](../../../contracts/otel/flaggo-service-observability-profile-v1.json),
whose strict shape is defined by
[`service-observability-profile-v1.schema.json`](../../../contracts/schemas/service-observability-profile-v1.schema.json).

Contract Service, Decision Service, OTel Ingestion, Evidence Materializer, and
Async Analysis implement the profile through shared .NET and Rust hosting
boundaries. New signals extend the registry and conformance checks before
service code emits them.

## Telemetry domains

Flaggo has two distinct OpenTelemetry domains:

| Domain | Producer | Purpose |
| --- | --- | --- |
| Application and evidence telemetry | Applications, SDK integrations, and the evidence pipeline | Route declared-authority telemetry and materialize evidence for later learning |
| Internal service telemetry | Flaggo services and workers | Diagnose service health, requests, queues, lifecycle transitions, and domain operations |

The domains can use the same OpenTelemetry protocols and backend, but they do
not share Resource identity. Service Resources must not contain
`flaggo.tenant`, `flaggo.application`, or `flaggo.environment`. Those
attributes are application-routing authority, not Flaggo service identity.

## Resource and scope identity

Every process emits one Resource containing:

- fixed `service.name`;
- `service.namespace = flaggo`;
- build-derived `service.version`; and
- one process-lifetime `service.instance.id`.

`deployment.environment.name` is optional deployment metadata. Each service
also uses the exact instrumentation scope registered by the profile. Scope
version equals the emitting package version. The reserved schema URL remains
unset until the schema is published and retrievable.

## Meaningful instrumentation boundaries

Automatic HTTP server and client instrumentation remains enabled where those
boundaries exist. Custom telemetry describes meaningful Flaggo operations, not
every method call.

Registered domain spans cover:

- contract validation, deployment, read, and catalog operations;
- candidate submission and activation;
- decision evaluation;
- OTLP inbox append and retention;
- materializer page processing and catalog refresh; and
- asynchronous analysis cycles and their observed session, evidence, artifact,
  provider and proposal actions.

Routine successful HTTP work does not need a duplicate informational log when
the HTTP span and metrics already describe it. Structured log events are
reserved for significant lifecycle changes, state transitions, rejections,
conflicts, progress milestones, and failures.

## Correlation

W3C `traceparent` is the distributed trace identity for synchronous service
calls. `X-Flaggo-Correlation-Id` remains the public request-correlation value
echoed by HTTP services; it may intentionally differ from the trace ID.

Important contract operations carry exact `flaggo.contract.name` and
`flaggo.contract.digest` values on spans and logs. Candidate, executable,
batch, cycle, attempt, and request-correlation identifiers may also appear on
spans and logs where the profile permits them.

Durable polling workers start new root spans for each bounded unit of work.
They correlate through exact domain identifiers instead of persisting trace
context in durable domain records or schemas.

## Cardinality and content safety

Metrics use only attributes marked as safe metric dimensions by the profile.
Exact contract, executable, candidate, batch, cycle, attempt, and correlation
identifiers are unbounded and must never become metric dimensions.

Telemetry may contain bounded operational metadata and hashed identifiers. It
must not contain complete contracts, runtime inputs, runtime decisions, OTLP
payloads, evidence observations, analysis prompts or model output, HTTP bodies
or header values, database statements, credentials, tokens, or secrets.

## Analysis progress

Async Analysis emits `flaggo.analysis.action.started` and
`flaggo.analysis.action.completed` logs plus `flaggo.analysis.action` spans.
An action is one observed operation, not an agent-authored progress message or
a durable workflow record. The action kind and outcome use the profile's
finite vocabularies. Exact contract, cycle, attempt and generated action UUIDs
correlate the observations; every new invocation, including a retry, has its
own action identity. Available provider tool-call/session references and trace
identity link provider observations to the same operation without duplicating
trusted custom-tool execution.

Producer ownership stays local:

| Boundary | Observations |
| --- | --- |
| Session lifecycle | Session creation and execution, including explicit domain terminal outcomes |
| Evidence capabilities | Schema discovery, committed cutoff/watermark, SQL and proposal submission |
| Agent workspace access | Meaningful artifact reads, writes and appends, not every metadata or directory operation |
| Provider adapter | Only supported provider request observations, never inferred thinking or private reasoning |

SQL telemetry includes the requested query's SHA-256 digest, committed
snapshot cutoff/watermark, measured duration and successful returned rows and
encoded result bytes. Returned rows and watermark are **not rows scanned**.
Failed queries retain their query/snapshot references and bounded failure
classification without raw database error text. Artifact operations expose
content digests and byte counts, not contents or paths. For append operations
these describe the appended fragment, not the complete resulting file;
unsuccessful content operations omit content digest/size. Metadata-only
operations and internal `.copilot`/`.git` content access are not observed.
Prompts, tool arguments/results,
model text and arbitrary provider event JSON are not part of this contract.
No action identifier or action metadata becomes a metric dimension; the
existing bounded cycle metrics remain sufficient for the timeline.
Producer correlation is bounded to 256 bytes for contract names and 128 bytes
for cycle/attempt identities. Invalid correlation suppresses public action
records with an explicit warning that contains no rejected values; it never
truncates domain identity or changes domain execution.

Start logs make ongoing operations observable before completed spans export.
A terminal observation without a start can describe its actual result but
cannot establish a missing start time. A start without a terminal means
completion was not observed; it is not proof of continued execution, success,
timeout or failure. Explicitly observed future interruption differs from a
process disappearing without exporting completion. Provider session activity
alone is not proof that a model request is pending.
The current Copilot adapter does not emit `provider.request` actions: its SDK
does not expose reliably paired concrete request terminals. Session and tool
actions remain observable, but time between them is not labeled model waiting.

The playground reconstructs a bounded last-known view through standard
observability APIs, grouped by exact cycle/attempt/action identities. Duplicate
and out-of-order observations cannot fabricate success; contradictory facts
remain uncertain. Query/history limits and backend stale/error states are
visible. Backend recovery refreshes the same view without losing inspection
focus or changing gameplay. Backend retention is the history boundary: no
durable action outbox, custom telemetry database, worker query server, or
complete-delivery guarantee is introduced.

Later dataset/Python and role workflows extend the registered action vocabulary
at their actual owned boundaries; the present single-agent workflow does not
invent those operations or roles.

## Export and failure isolation

Service telemetry exports logs, metrics, and traces as OTLP/HTTP binary
Protobuf. Structured console logging remains available. OTLP export is enabled
only through standard `OTEL_EXPORTER_OTLP_*` endpoint configuration.

Invalid exporter configuration fails startup explicitly. Once configuration
is valid, an unavailable telemetry backend cannot change domain outcomes:

- Contract Service acceptance or activation does not depend on export;
- Decision Service results do not depend on export;
- OTel Ingestion acknowledgement still depends only on its durable inbox
  boundary;
- Evidence Materializer progress does not depend on export; and
- Async Analysis outcomes do not depend on export.

Export queues, retries, timeouts, and shutdown flushes are bounded. OTel
Ingestion rejects configuration that points its exporter at its own local
receiver endpoint, preventing recursive self-ingestion.

The profile deliberately does not select a vendor or local observability
backend. The local playground deployment owns that choice and pins the
compatible backend configuration.

## Health boundaries

Contract Service, Decision Service, and OTel Ingestion retain their existing
HTTP liveness and readiness endpoints. Evidence Materializer and Async Analysis
remain background workers and do not gain HTTP servers solely for
observability. Their health is inferred from process lifecycle, startup and
shutdown events, progress signals, error signals, and work-age metrics.

## Implementation constraints

Each service implementation must:

1. use the registered Resource and instrumentation scope;
2. emit only registered custom attributes, spans, log events, and metrics;
3. preserve automatic protocol instrumentation where applicable;
4. replace obsolete ad hoc operational event formats rather than maintaining
   compatibility parsers;
5. keep exporter failure outside domain success and failure decisions; and
6. extend the profile and conformance checks before introducing a new
   cross-service semantic.
