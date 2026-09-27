# Cross-Module Tests

This directory contains integration and host-boundary tests that span module
boundaries. Unit tests remain beside the module or package they verify.

- `Flaggo.Core.Tests` verifies contract validation and identity, expression
  compilation, immutable stores, activation concurrency, and pure runtime
  evaluation.
- `Flaggo.ContractService.Tests` dispatches the Management API v3 fixtures
  through a real Contract Service host.
- `Flaggo.DecisionService.Tests` dispatches the Runtime API v3 fixtures through
  a real Decision Service host.

Tests use deterministic local adapters and temporary SQLite databases. Cloud
services are never required for the default test path.
