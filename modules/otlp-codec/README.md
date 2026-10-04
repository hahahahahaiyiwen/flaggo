# OTLP Codec

This Rust crate is the shared decoding boundary for accepted OTLP export
requests. The receiver uses it before durable acknowledgement, and the Evidence
Materializer uses the same decoder for retained payloads.

For Protobuf JSON, the codec:

- accepts both canonical lower-camel field names and original protobuf field
  names at every nesting level;
- rejects exact duplicates, duplicate aliases, and ambiguous oneof members;
- rejects unknown request-envelope fields;
- follows the OpenTelemetry generated model's forward-compatible policy of
  ignoring unknown nested fields; and
- leaves the original request bytes unchanged for inbox persistence.

Binary protobuf decoding uses the same generated OTLP request types.
