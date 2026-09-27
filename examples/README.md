# Examples

The current examples use v3 `DecisionContract` publication, exact-version
runtime calls, and application-owned OpenTelemetry.

Examples demonstrate Flaggo integrations without becoming production module
dependencies.

Small, self-contained examples may live here. Larger showcase applications,
including the Tetris frontend, may remain external repositories and pin a
published SDK/contract version. Every example must document a cloud-free local
run path.

`tetris-integration` starts the Contract and Decision Services against an
isolated SQLite database. It validates and publishes one contract, executes
its authored rules through the TypeScript SDK, verifies default behavior for
missing attributes, and confirms that retired v1 request fields are absent.

`adaptive-worker` publishes one contract and processes a deterministic
in-memory queue through the exact-version Decision Service. It covers both
default and authored-rule decisions and records native application OTel logs
without cloud services.
