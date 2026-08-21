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
ignored/accounted bucket, and manifest cases without an explicit plan fail.

Registry integration tests use unique files below `TestResults` and prove that
an approval committed through a control-plane host becomes decidable through
an already-created, separately configured data-plane host.

## Decision-service CI diagnostics

The `Contracts` workflow runs the solution test suite with a 10-minute test
step limit and a 15-minute job limit, so a non-terminating test host cannot
consume GitHub's six-hour hosted-runner limit.

This safeguard was added after multiple Ubuntu runs left the `dotnet test`,
MSBuild, VSTest, and testhost process tree alive until GitHub cancelled it at
six hours. Identical code and runner images passed on rerun, and the current
suite passed in a native Ubuntu container, so the stall is intermittent rather
than a deterministic test failure. The historical runs had only the default
summary logger, so they did not retain enough evidence to attribute the stall
to one test.

CI now emits individual test progress, enables VSTest hang collection with a
two-minute test timeout, and writes platform diagnostics to the runner's
temporary directory. A failed test step uploads those diagnostics, including
the sequence and mini dump when the hang collector can produce them. The
outer step timeout remains authoritative if VSTest itself stops responding;
GitHub hosted-runner cleanup has been observed terminating every remaining
`dotnet` child process after cancellation.
