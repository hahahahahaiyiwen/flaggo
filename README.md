# Flaggo

Flaggo is a decisioning system for bounded runtime variables. It gives
application code an explicit way to deploy a versioned `DecisionContract`,
activate one immutable `DecisionExecutable`, and request a deterministic
result for an exact contract version.

Phase 3 implements complete SDK input construction, contract acceptance,
default and deterministic authored-executable generation, atomic activation,
and stateless runtime evaluation. Exposure telemetry, evidence ingestion and
correlation, and asynchronous evidence-based generation remain future work.

## Target product model

```text
DecisionContract
  -> Contract Acceptance
  -> Executable Generation
  -> CandidateExecutable
  -> Activation
  -> ActiveExecutable

ActiveExecutable + complete RuntimeInput
  -> deterministic RuntimeDecision
  -> application applies result
  -> future exposure, evidence, and learning loop
```

Activation is the only transition that grants runtime authority. The initial
learning declaration uses `mode: auto-activation` to define how a future valid
evidence-generated candidate would attempt activation. Phase 3 stores that
declaration but does not run evidence ingestion or a learning worker. Runtime
reads only the activation mapping.

The repository implements this v3 model through Contract Service, Decision
Service, reusable domain/store modules, the TypeScript SDK, and real-host
examples. The pre-v3 service stack has been removed.

## Start here

- [Documentation home](docs/README.md)
- [Manifesto](docs/MANIFESTO.md)
- [Architecture overview](docs/design/architecture/OVERVIEW.md)
- [Project roadmap](https://github.com/users/hahahahahaiyiwen/projects/3)
- [Contributing](CONTRIBUTING.md)

## Quickstart

Run the interactive Tetris example against real local Contract, Decision, and
OTel Ingestion services:

```powershell
npm ci
dotnet restore Flaggo.slnx --configfile NuGet.config
npm run tetris:flaggo
```

The launcher builds the .NET Contract and Decision services and Rust OTel
Ingestion, creates an isolated SQLite database, deploys the Tetris contract,
and removes its local state when the game exits. Tetris-owned OpenTelemetry
providers export application logs, metrics, and traces directly to ingestion.
No cloud account or Collector process is required.

Validate the executable contracts independently:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

`python tools\dev.py serve` starts only the fixture server used for contract
and SDK conformance development; it does not start Contract Service or
Decision Service.

## Repository layout

```text
apps/          Runnable service hosts
modules/       Internal business capabilities and owned ports
packages/      Reusable SDK and data-contract packages
contracts/     OpenAPI, JSON Schema, OTel profiles, fixtures, and conformance
tests/         Cross-module and end-to-end verification
examples/      Integrations and showcase links
deploy/        Fixture-container assets; production service topology is deferred
tools/         Repository development commands
docs/          Product, architecture, scenarios, and boundary designs
```

Logical architecture is defined by service and store ownership, not by the
current assembly count. Prefer the smallest complete end-to-end behavior over
speculative platform breadth.
