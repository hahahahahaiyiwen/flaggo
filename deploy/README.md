# Deployment

The current Compose asset runs only the fixture server used by contract and
SDK conformance development. It does not deploy Contract Service or Decision
Service and is not a production topology.

Use `npm run tetris:flaggo` or `npm run test:adaptive-worker` for cloud-free
real-host execution. Both applications own their OpenTelemetry providers and
export OTLP/HTTP JSON directly to dynamically assigned
Rust OTel Ingestion endpoints. A separate Collector process is optional and is
not part of the Phase 4 deployment assets. Production service images,
ingestion authentication, and orchestration remain deferred. Future assets
must keep services independently deployable and must use environment
configuration without committing credentials.
