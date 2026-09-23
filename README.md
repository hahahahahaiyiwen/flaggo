# Flaggo

Flaggo is a closed-loop decisioning system for bounded runtime variables. It
gives application code an explicit way to register a versioned decision
definition, activate approved authority, request a deterministic value, and
attribute outcomes after the application confirms use.

The application owns execution and its OpenTelemetry pipeline. Flaggo owns the
registered definition, approved authority, deterministic constraint evaluation,
durable decision and exposure records, and future asynchronous candidate path.

## Current product slice

```text
decision definition + initial authority candidate
  -> Contract Service validation and approval
  -> State Store activation
  -> Decision Service execution
  -> deterministic constraints + durable decision record
  -> confirmed exposure
  -> attributed outcome
```

Current authority is either an approved fixed value or an approved numeric
rule. Proposal generation, experiments, rollouts, and other proposal-managed
workflows remain future work.

The first scenario governs `tetris.dropInterval`, returning an approved
`750ms` or `850ms` value from live game inputs with an `800ms` fallback.

## Start here

- [Documentation home](docs/README.md)
- [Manifesto](docs/MANIFESTO.md)
- [Architecture overview](docs/architecture/OVERVIEW.md)
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
