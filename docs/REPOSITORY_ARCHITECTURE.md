# Repository Architecture

Flaggo uses a modular monorepo for its open-source core. Repository boundaries
do not define runtime boundaries: the control plane, data plane, workers, and
operator console may be built and deployed independently while sharing one
versioned source tree.

## Why a monorepo

The MVP contracts, SDK, APIs, and domain modules will change together. Keeping
them together provides atomic changes, one conformance gate, reproducible local
development, and a simpler path for new contributors. It also avoids publishing
temporary packages merely to coordinate changes between immature components.

## Top-level boundaries

| Path | Ownership |
| --- | --- |
| `apps/` | Independently runnable Flaggo processes and user interfaces |
| `modules/` | Business capabilities with module-owned interfaces |
| `packages/` | Published or reusable client and contract packages |
| `contracts/` | Language-neutral OpenAPI, schemas, fixtures, and conformance |
| `tests/` | Cross-module and end-to-end verification |
| `examples/` | Small integrations and links to external showcase applications |
| `deploy/` | Container and local deployment assets |
| `tools/` | Repository development and automation commands |
| `docs/` | Product, architecture, and component design sources |

## Dependency rules

1. Business behavior depends on module-owned interfaces, not infrastructure.
2. Interface dependencies use constructor injection.
3. `packages/shared-contracts` contains data contracts, not a global interface
   collection.
4. Applications compose modules and adapters; modules do not depend on apps.
5. Cross-process behavior is governed by the executable artifacts in
   `contracts/`.
6. Every module boundary maintains its own `README.md` and focused tests.

## When to split a repository

A component moves out only when it has a demonstrably independent lifecycle:

- independent maintainers or governance,
- a materially different release cadence,
- a security or access-control boundary,
- CI scale that makes the shared repository impractical, or
- a stable contract with infrequent coordinated changes.

Deployment isolation alone is not a reason to split source repositories.
External showcase applications, such as the Tetris integration, may remain in
their own repositories.

## Initial implementation shape

The first server implementation should be a modular system with separate
control-plane and data-plane hosts. Registry, lifecycle, policy, state, evidence,
reasoning, and audit remain explicit modules behind owned ports. They may use
in-memory adapters during the local MVP and become separate services only when
operational requirements justify that change.

`modules/lifecycle` owns proposal review and activation orchestration through
`IProposalGovernance`, independently of runtime reasoning. It consumes
registry-owned runtime/intelligence projections, lifecycle policy, proposal
evidence, trusted actor resolution, and state commits. Interfaces remain with
their owning module; cross-module proposal/state/policy/receipt data lives in
`packages/shared-contracts`.

Lifecycle audit semantics and its read port belong to audit; state owns the
atomic journal/state persistence boundary and depends on audit integrity
validation. This physically co-locates lifecycle audit and authority without
introducing a distributed transaction. Runtime still consumes read-only state,
runtime policy, and separate decision/exposure audit. Apps configure adapters
and authentication, not governance logic.
