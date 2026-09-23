# Contributing to Flaggo

Flaggo is early-stage. Design clarity, executable contracts, and a portable
local workflow take priority over implementation volume.

Start with the [documentation home](docs/README.md) and select accepted work
from [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3).

## Development setup

Requirements:

- Python 3.11 or newer
- Git
- Docker with Compose (optional)

Install the current contract tooling and run the baseline:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

Start the fixture-backed API:

```powershell
python tools\dev.py serve
```

No cloud account or external service is required.

## Change expectations

- Implement the smallest complete behavior that satisfies the accepted
  contract.
- Update executable contracts and fixtures when wire behavior changes.
- Keep module interfaces owned by the module where behavior belongs.
- Use constructor injection for cross-module dependencies.
- Add or update behavior tests before changing implementation behavior.
- Update a module's `README.md` when its boundary, invariants, or dependencies
  change.
- Remove obsolete paths instead of preserving compatibility layers unless an
  accepted requirement explicitly demands compatibility.
- Keep commits focused and avoid unrelated formatting or generated output.

## Repository architecture

Flaggo uses a modular monorepo for its open-source core. Repository boundaries
do not define runtime boundaries: control-plane hosts, data-plane hosts,
workers, and operator interfaces may be built and deployed independently while
sharing one versioned source tree.

### Why a monorepo

The contracts, SDK, APIs, domain modules, examples, and tests still change
together. A monorepo provides atomic changes, one conformance gate,
reproducible local development, and a simpler contributor path without
publishing temporary coordination packages.

### Top-level boundaries

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
| `docs/` | Product, architecture, roadmap, and component design sources |

### Dependency rules

1. Business behavior depends on module-owned interfaces, not infrastructure.
2. Interface dependencies use constructor injection.
3. `packages/shared-contracts` contains data contracts, not a global interface
   collection.
4. Applications compose modules and adapters; modules do not depend on apps.
5. Cross-process behavior is governed by executable artifacts in `contracts/`.
6. Every module boundary maintains focused documentation and tests.

### Initial implementation shape

The first server implementation remains modular, with separate control-plane
and data-plane hosts. Registry, policy, state, evidence, reasoning, and audit
stay explicit modules behind owned ports. Local adapters may use simple
persistence; separate services are justified only by operational requirements.

### When to split a repository

A component moves out only when it has a demonstrably independent lifecycle:

- independent maintainers or governance;
- a materially different release cadence;
- a security or access-control boundary;
- CI scale that makes the shared repository impractical; or
- a stable contract with infrequent coordinated changes.

Deployment isolation alone is not a reason to split source repositories.
External showcase applications may remain separate when they have an
independent lifecycle.

## Pull requests

Describe the behavior changed, the boundary that owns it, and the validation
performed. Contract changes must keep OpenAPI, schemas, fixtures, docs, and the
conformance gate aligned.
