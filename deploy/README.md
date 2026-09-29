# Deployment

The current Compose asset runs only the fixture server used by contract and
SDK conformance development. It does not deploy Contract Service or Decision
Service and is not a production topology.

Use `npm run tetris:flaggo` or `npm run test:adaptive-worker` for cloud-free
real-host execution. `otel-collector-flaggo-local.yaml` is a minimal local
Collector example that receives OTLP HTTP logs, metrics, and traces on
`localhost:4318` and forwards them as OTLP HTTP JSON to `Flaggo.OtelIngestion`
on `localhost:5090` at `/v1/logs`, `/v1/metrics`, and `/v1/traces`. The
Collector's default gzip compression is supported by ingestion. Production
service images, ingestion authentication, and orchestration remain deferred.
Future assets must keep services independently deployable and must use
environment configuration without committing credentials.
