# OpenTelemetry ingestion design

## Purpose

OTel Ingestion is the server ingress boundary that accepts application-owned
OpenTelemetry data and writes normalized observations to Evidence Store.

The application owns instrumentation APIs, providers, sampling, exporters, and
Collector configuration. Flaggo does not require or configure a second client
exporter.

## Responsibilities

OTel Ingestion:

- accepts supported OTLP metrics, traces/spans, and structured logs/events;
- authenticates the application and environment scope;
- resolves declared evidence bindings from Contract Store;
- validates signal kind, selected fields, type, unit, target mapping, and scope;
- records provenance, timestamps, sampling/coverage metadata, and rejection
  reasons; and
- appends normalized immutable observations to Evidence Store.

It does not execute online decisions, activate authority, invent missing values,
or reinterpret unsupported projections as valid evidence.

## Flow

```text
application-owned OTel pipeline
  -> OTLP transport
  -> scope and binding validation
  -> normalized observation
  -> Evidence Store
```

Live runtime inputs still travel with a decision request. Asynchronous
telemetry export is not a substitute for current request state.

## Failure behavior

Unsupported projections, missing required attributes, type or unit mismatch,
unauthorized scope, invalid target mapping, and unusable sampling/coverage are
explicit rejected or unsuitable observations. They never become zero-valued or
fabricated successful evidence.

## Current implementation mapping

Issue #44 owns the client manifest and OTel binding contract plus the smallest
local projection slice. This document defines the server ingress boundary and
does not change #44's client ownership.

## Related documents

- [Decision evidence](../../architecture/EVIDENCE.md)
- [Evidence Store](../evidence-store/README.md)
- [Contract Service](../contract-service/README.md)
