# Service Observability

This Rust module composes the shared operational OpenTelemetry boundary for
Flaggo services and workers. It owns:

- the `flaggo` service Resource identity;
- one process-lifetime `service.instance.id`;
- JSON structured console output;
- W3C trace-context propagation;
- optional OTLP/HTTP binary Protobuf exporters for logs, metrics, and traces;
- standard `OTEL_EXPORTER_OTLP_*` validation;
- rejection of authority identity in `OTEL_RESOURCE_ATTRIBUTES`;
- bounded provider shutdown; and
- OTel Ingestion self-export rejection.

It does not define domain spans, events, or metrics. Each app emits only the
signals registered for its instrumentation scope by
[`flaggo-service-observability-profile-v1.json`](../../contracts/otel/flaggo-service-observability-profile-v1.json).

No exporter is created unless `OTEL_EXPORTER_OTLP_ENDPOINT` or the matching
signal-specific endpoint is configured. Protocol overrides, when present, must
be `http/protobuf`.
