# Cross-Module Tests

This directory is reserved for integration, contract, and end-to-end tests
that span module boundaries. Unit tests remain beside the module or package
they verify.

Cross-module tests should use real composition with deterministic local
adapters. Cloud services are never required for the default test path.
