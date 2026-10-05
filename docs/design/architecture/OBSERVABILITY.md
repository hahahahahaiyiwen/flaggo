# Internal service observability

## Status and authority

This document defines operational observability for Flaggo-owned services and
workers. The machine-readable authority is
[`flaggo-service-observability-profile-v1.json`](../../../contracts/otel/flaggo-service-observability-profile-v1.json),
whose strict shape is defined by
[`service-observability-profile-v1.schema.json`](../../../contracts/schemas/service-observability-profile-v1.schema.json).

Contract Service, Decision Service, OTel Ingestion, and Evidence Materializer
implement the profile through shared .NET and Rust hosting boundaries. Async
Analysis remains a registered scope but must adopt the same boundary when its
app is integrated. New signals extend the registry and conformance checks
before service code emits them.

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
- asynchronous analysis cycles.

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
