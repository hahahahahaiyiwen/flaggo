# Examples

The current examples use v3 `DecisionContract` deployment, exact-version
runtime calls, and application-owned OpenTelemetry.

Examples demonstrate Flaggo integrations without becoming production module
dependencies.

Small, self-contained examples may live here. Larger showcase applications,
including the Tetris frontend, may remain external repositories and pin a
released SDK and exact deployed contract version. Every example must document a cloud-free local
run path.

`tetris-integration` is a playable terminal Tetris app. It runs independently
with local level-based gravity, or optionally starts local Contract and
Decision Services and delegates the drop interval through the TypeScript SDK.
Its separate automated harness deploys the contract, executes
the authored rules, verifies default behavior for missing attributes, and
confirms that retired v1 request fields are absent.

`adaptive-worker` deploys one contract and processes a deterministic
in-memory queue through the exact-version Decision Service. It covers both
default and authored-rule decisions and records native application OTel logs
without cloud services.
