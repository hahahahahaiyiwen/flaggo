# Examples

Examples demonstrate Flaggo integrations without becoming production module
dependencies.

Small, self-contained examples may live here. Larger showcase applications,
including the Tetris frontend, may remain external repositories and pin a
published SDK/contract version. Every example must document a cloud-free local
run path.

`tetris-integration` is the Phase 3 local reference integration. It contains
the canonical definition bundle, trusted approval-aware bootstrap, governed
state activation template, real-host SDK/REST harness, and local audit and
telemetry inspection commands. It intentionally contains no frontend game
behavior. Its strategy policy requires confidence evidence and the harness
proves missing active-strategy evidence fails closed.

`adaptive-worker` is the Phase 2.5 SDK-to-service acceptance application. It
extracts its decision contract from TypeScript, registers it with the local
control plane, processes a deterministic in-memory queue through the data
plane, confirms exposures only after applying each batch size, and records
local telemetry without cloud services.
