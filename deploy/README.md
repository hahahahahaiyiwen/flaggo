# Deployment

The current Compose asset runs only the fixture server used by contract and
SDK conformance development. It does not deploy Contract Service or Decision
Service and is not a production topology.

Use `npm run tetris:flaggo` or `npm run test:adaptive-worker` for cloud-free
real-host execution. Production service images and orchestration remain
deferred; future assets must keep both services independently deployable and
must use environment configuration without committing credentials.
