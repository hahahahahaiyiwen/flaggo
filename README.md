# Flaggo

Flaggo is a closed-loop decisioning system for bounded runtime variables. It
gives application code an explicit way to deploy a versioned
`DecisionContract`, activate one immutable `DecisionExecutable`, request a
deterministic result, and correlate outcomes after the application reports
exposure.

The application and SDK own complete input construction, application of
returned results, and OpenTelemetry exposure emission. Flaggo owns contract
acceptance, executable generation and validation, atomic activation, stateless
runtime evaluation, evidence ingestion, and asynchronous learning.

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
  -> applied exposure and correlated evidence
```

Activation is the only transition that grants runtime authority. The initial
learning policy is `mode: auto-activation`: a valid evidence-generated
candidate from the current learning head immediately attempts atomic
activation. Runtime still reads only the activation mapping.

The repository implements this v3 model through Contract Service, Decision
Service, reusable domain/store modules, the TypeScript SDK, and real-host
examples. The former v1 control-plane and data-plane stacks have been removed.

## Start here

- [Documentation home](docs/README.md)
- [Manifesto](docs/MANIFESTO.md)
- [Architecture overview](docs/design/architecture/OVERVIEW.md)
- [Project roadmap](https://github.com/users/hahahahahaiyiwen/projects/3)
- [Contributing](CONTRIBUTING.md)

## Quickstart

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

Start the fixture-backed local API:

```powershell
python tools\dev.py serve
```

No cloud account is required.

## Repository layout

```text
apps/          Runnable service hosts
modules/       Internal business capabilities and owned ports
packages/      Reusable SDK and data-contract packages
contracts/     OpenAPI, JSON Schema, fixtures, and conformance
tests/         Cross-module and end-to-end verification
examples/      Integrations and showcase links
deploy/        Container and deployment assets
tools/         Repository development commands
docs/          Product, architecture, scenarios, and boundary designs
```

Logical architecture is defined by service and store ownership, not by the
current assembly count. Prefer the smallest complete end-to-end behavior over
speculative platform breadth.
