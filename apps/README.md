# Applications

`apps` contains application composition roots. An app chooses concrete
adapters, configuration, lifecycle, and hosting for one cohesive operational
boundary; reusable domain and store contracts remain under `modules`.

The canonical relationship between clients, APIs, workers, and stores is
[Application boundaries and lifecycle](../docs/design/architecture/APP_BOUNDARIES.md).
The shared operational telemetry contract is
[Internal service observability](../docs/design/architecture/OBSERVABILITY.md).

## Current composition roots

| App | Execution model | Boundary |
| --- | --- | --- |
| [Contract Service](contract-service/README.md) | Web API | Contract acceptance, executable validation, and activation authority |
| [Decision Service](decision-service/README.md) | Web API | Stateless exact-version runtime evaluation |
| [OTel Ingestion](otel-ingestion/README.md) | Web API plus background worker | Append-before-acknowledgement OTLP ingress and Raw OTLP Inbox retention |
| [Evidence Materializer](evidence-materializer/README.md) | Background worker | Forward-only Raw OTLP Inbox to Evidence Store projection |

[Operator Console](operator-console/README.md) is a planned client, not a
current runnable server composition root. Async Analysis is also planned and
will receive its own app boundary when implemented.

## Boundary rules

- Diagram nodes are conceptual components, not process or replica counts.
- A rectangle denotes a request-serving Web API. A diamond denotes
  asynchronous or periodic work. OTel Ingestion currently hosts both roles in
  one binary.
- Apps do not reference another app project. Cross-app communication uses an
  explicit API or domain/store contract.
- Physical co-location does not transfer ownership. The current local
  deployment shares SQLite files while each store module owns its schema and
  each app has a distinct semantic access role.
- Contract and Decision APIs never depend on telemetry ingestion,
  materialization, evidence freshness, or analysis.
- Every runnable app emits profile-aligned structured console logs, traces,
  and metrics. OTLP/HTTP export is enabled only when a standard
  `OTEL_EXPORTER_OTLP_*_ENDPOINT` or `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
- Invalid exporter configuration fails startup. An unavailable configured
  backend does not change request, persistence, materialization, or activation
  outcomes.
- The pre-v3 service stack and compatibility routes are not part of the
  current architecture.
