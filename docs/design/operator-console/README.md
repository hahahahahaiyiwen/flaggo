# Operator Console Design

## Purpose

The operator console is the human governance surface for Flaggo. It lets operators inspect decisions, active state, evidence quality, policy outcomes, audit records, overrides, and rollback controls.

The full console is not required for the MVP. The MVP should still expose enough local audit and state visibility that a future console has clear API and data seams.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP expectation

The MVP does not need a polished UI. It should provide inspectable outputs through:

- structured audit records,
- local logs or file audit sink,
- active state fixtures,
- documented API responses,
- optional simple debug endpoint or script.

## Future responsibilities

The future operator console should support:

- list decision keys and definitions,
- inspect active contracts and lifecycle state,
- inspect active values and active strategies,
- view recent decisions and audit records,
- view evidence quality and confidence,
- pause or resume a surface/scope,
- set or clear operator overrides,
- approve, reject, or roll back proposals,
- inspect fallback and policy reason trends.

## MVP non-goals

- UI implementation.
- Authentication and authorization model.
- Multi-tenant administration.
- Rich audit search.
- Approval workflow UI.

These should be added after the local end-to-end Tetris MVP proves the runtime path.
