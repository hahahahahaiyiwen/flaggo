# Pinned OTLP protocol definitions

These eight unmodified protobuf files are from
[`open-telemetry/opentelemetry-proto` v1.11.0](https://github.com/open-telemetry/opentelemetry-proto/tree/v1.11.0),
commit `790608c4d51e6ffc12210b541e8514cbed9e91a4`, under the included Apache-2.0
[license](LICENSE).

Only common/resource types and the stable metrics, traces, and logs export
messages are included. `Grpc.Tools` generates C# message types at build time;
generated files are not checked in and no gRPC service is exposed. Domain
modules do not depend on generated protobuf types.

Update the release pin and all eight files together, preserve upstream
licensing, and rerun OTLP host-boundary and Collector integration coverage.
