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
Its Flaggo mode shares the playground's adaptive contract and session-owned
experience tracker/provider. The automated harness runs genuine greedy
episodes, verifies exposure-correlated four-gauge evidence, SDK/REST parity,
restart persistence, and retired-field absence. Its opt-in live-agent extension
uses the same nine-game training protocol and verifies Candidate auto-activation.

`tetris-playground` composes that headless game boundary with all real Flaggo
services, a pinned Collector and Grafana LGTM, and a browser observer. Its
default live mode uses credentialed production Copilot analysis, one dominant
human game, and ten initially idle real-time greedy slots contributing
attributed evidence. The separate `--mode=benchmark` launch defaults to the
deterministic fitter and measures matched held-out improvement under the
actually activated executable. Live learning does not guarantee a Candidate
or improved human enjoyment. See
[the playground guide](tetris-playground/README.md).

`adaptive-worker` deploys one contract and processes a deterministic
in-memory queue through the exact-version Decision Service. It covers both
default and authored-rule decisions and records native application OTel logs
without cloud services.
