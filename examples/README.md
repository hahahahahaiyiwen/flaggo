# Examples

The examples use manifest-first publication, key-based runtime calls, and
application-owned OTel. Executable server terminology/alignment remains #49;
manifest initial-authority publication remains #40, followed by #41's final
bundle-approved Tetris run. Current trusted local state bootstrap is not an
activation-ready registration receipt.

Examples demonstrate Flaggo integrations without becoming production module
dependencies.

Small, self-contained examples may live here. Larger showcase applications,
including the Tetris frontend, may remain external repositories and pin a
published SDK/contract version. Every example must document a cloud-free local
run path.

`tetris-integration` is the current Phase 3 local integration. It contains
the canonical definition bundle, trusted approval-aware bootstrap, governed
state activation template, real-host SDK/REST harness, and local audit and
telemetry inspection commands. It intentionally contains no frontend game
behavior. Four plain live inputs drive exact `850ms`/`750ms` branches with
null learned confidence. It verifies a separate `800ms` cooldown fallback and
explicit confirmation followed by an ordinary application OTel outcome log.

`adaptive-worker` is the Phase 2.5 SDK-to-service acceptance application. It
compiles one JSON manifest into a bundle and typed catalog. Trusted tooling
publishes the contract separately; the runtime processes a deterministic
in-memory queue through the data
plane, confirms exposures only after applying each batch size, and records
native application OTel logs without cloud services.

`otel-evidence` sends actual application OTel metrics, traces/span events, and
structured logs through a pinned stock Collector. Existing export pipelines
remain intact while a new branch materializes four required evidence inputs
and a confirmed outcome. It verifies durable source provenance and runtime
consumption without replacing telemetry producers.
