# OpenTelemetry contracts

This directory contains Flaggo's OpenTelemetry semantic schema and OTLP/HTTP
capability profile. It does not fork or copy the upstream OTLP protobuf model.

## Flaggo telemetry schema

`flaggo-telemetry-schema-1.0.0.yaml` is the source for the immutable
OpenTelemetry Schema File reserved for:

```text
https://flaggo.dev/schemas/telemetry/1.0.0
```

The SDK must not emit this instrumentation-scope `schemaUrl` until the file is
published and retrievable there.

It versions Flaggo event names and attributes for schema-aware telemetry
consumers. The complete current event shapes are defined separately by
`../schemas/telemetry-events-v1.schema.json`; an OpenTelemetry Schema File
describes transformations between semantic-convention versions rather than the
full valid shape of a `LogRecord`.

## Flaggo OTLP/HTTP profile

`flaggo-otlp-http-profile-v1.json` pins the OTLP specification and protobuf
definitions that remain authoritative for payloads. The Flaggo profile only
selects signals, endpoints, encodings, compression, limits, response behavior,
authentication phase, and unsupported transports.

The profile deliberately has no custom request or response schema for
`/v1/logs`, `/v1/metrics`, or `/v1/traces`. Standard OpenTelemetry exporters
must be able to use these endpoints without a Flaggo-specific transport.

Phase 4 requires both standard OTLP/HTTP encodings (`application/json` and
`application/x-protobuf`) with identity or gzip request compression. A
successful response acknowledges that the complete valid export request was
durably appended to the Raw OTLP Inbox. Item-level decoding, selection, and
materialization happen asynchronously, so the receiver does not return OTLP
partial-success responses.

The offline conformance gate validates the profile and its golden fixtures.
Passing that gate establishes artifact consistency; real-host interoperability
must additionally dispatch the fixtures through OTel Ingestion.
