# Contributing to Flaggo

Flaggo is early-stage. Design clarity, executable contracts, and a portable
local workflow take priority over implementation volume.

Start with the [documentation home](docs/README.md) and select accepted work
from [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3).

## Development setup

Requirements:

- Python 3.11 or newer
- .NET SDK 10
- Node.js 20 or newer
- Rust using the repository-pinned toolchain
- Git
- Docker with Compose (optional)

Install the current contract tooling and run the baseline:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

Install JavaScript dependencies, restore through the repository NuGet
configuration, and run a real-host smoke path:

```powershell
npm ci
dotnet restore Flaggo.slnx --configfile NuGet.config
npm run test:adaptive-worker
```

No cloud account or external service is required.

Build and verify the Rust workspace independently:

```powershell
npm run build:rust
npm run check:rust
```

For fixture-only contract and SDK development, run:

```powershell
python tools\dev.py serve
```

That command does not start Contract Service or Decision Service.

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
do not define runtime boundaries: Contract Service, Decision Service, future
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
| `deploy/` | Fixture-container assets; production service topology remains deferred |
| `tools/` | Repository development and automation commands |
| `docs/` | Product, architecture, scenario, and service/store design sources |

### Dependency rules

1. Business behavior depends on module-owned interfaces, not infrastructure.
2. Interface dependencies use constructor injection.
3. Language-neutral wire contracts live under `contracts/`; language clients
   and services validate against those same artifacts.
4. Applications compose modules and adapters; modules do not depend on apps.
5. Cross-process behavior is governed by executable artifacts in `contracts/`.
6. Every module boundary maintains focused documentation and tests.

### Portability boundaries

Flaggo must remain runnable without a managed cloud dependency. Domain and API
logic use provider-neutral contracts and standard protocols; provider SDKs
belong only in adapters at application composition boundaries.

Cross-cutting infrastructure is configured at application composition roots.
Business capabilities use module-owned ports when substitutability is needed;
the current stack injects `TimeProvider`, `IContractVersionStore`, and
`IExecutableStore` rather than defining speculative shared-provider
interfaces. Logical ownership follows Contract Service, Decision Service,
Contract Store, and Executable Store.

| Concern | Local implementation | Optional cloud adapter |
| --- | --- | --- |
| Configuration | Environment variables or explicit local files | Provider configuration service |
| Secrets | Environment variables or local development secret store | Provider secret manager |
| Contract and executable stores | SQLite | Managed SQL or document store |
| Runtime evaluation | In-process bounded evaluator | Independently scaled Decision Service |
| Evidence transport | Application-owned OpenTelemetry pipeline | Managed telemetry or analytics pipeline |

Public APIs, DecisionContracts, executable semantics, and durable store schemas
must remain usable without a cloud account. New providers add adapters behind
existing ports instead of changing core contracts.

### Initial implementation shape

The executable server stack has Contract Service, Decision Service, Contract
Store, Executable Store, a bounded expression compiler, and a stateless
evaluator. Contract and Decision Services share a configured SQLite database
while each store owns its tables and schema version.

OTel Ingestion and the Evidence Store form the Phase 4 telemetry foundation.
Asynchronous analysis remains a separately delivered component. These
capabilities may generate candidate executables, but they do not participate in
the synchronous decision path.

### Parallel contract implementation

Executable API artifacts change before or with client and service
implementations. The management client validates, deploys, and reads
immutable DecisionContract versions. The runtime client binds names to exact
contract digests, constructs complete RuntimeInput values, and preserves input
bytes across retries. SDK fallback remains outside the current contract.

Contract Service owns acceptance, default/authored executable generation,
activation, and management projections. Decision Service validates complete
input, resolves one active executable for the exact contract digest, evaluates
it, and returns a RuntimeDecision without persisting request state.

Both tracks test against the same OpenAPI documents, schemas, fixtures, and
conformance suites. Each implementation branch records the contract revision
it implements. A contract-breaking change uses a dedicated contract pull
request that updates executable artifacts, compatibility notes, fixtures, and
both tracks' conformance coverage. Tracks merge small vertical increments and
run cross-track contract tests continuously; end-to-end integration starts as
soon as one real-host exact-version decision call can complete.

Extensions must use the owning seam instead of bypassing it:

- add wire behavior through an accepted OpenAPI/schema/fixture change;
- add executable behavior through the contract, compiler, and evaluator
  boundaries;
- add storage through Contract Store or Executable Store ports;
- add asynchronous analysis as an authorized candidate producer feeding the
  Contract Service activation boundary;
- add telemetry transports through the application SDK/OpenTelemetry pipeline,
  not the runtime decision client; and
- add cloud providers through adapters behind existing ports.

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
