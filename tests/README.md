# Cross-Module Tests

This directory is reserved for integration, contract, and end-to-end tests
that span module boundaries. Unit tests remain beside the module or package
they verify.

Cross-module tests should use real composition with deterministic local
adapters. Cloud services are never required for the default test path.

`Flaggo.Decisioning.Tests` includes executable host-boundary tests and a
manifest-driven service conformance suite. Every HTTP fixture is dispatched
through its owning `WebApplicationFactory` host with explicit authentication,
clock, state, approval, exposure, idempotency, failure, or readiness setup.
Tests assert status, required headers, and response semantics. SDK-local and
schema-negative cases have named executable boundary checks rather than an
ignored/accounted bucket, and a new manifest case fails until it receives an
explicit plan.

Registry integration tests use unique files below `TestResults` and prove that
an approval committed through a control-plane host becomes decidable through
an already-created, separately configured data-plane host.
