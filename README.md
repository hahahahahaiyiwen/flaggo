# Flaggo

Flaggo is a policy-first decisioning control plane with a runtime decision
provider. It gives application code an explicit, governed way to request
bounded runtime values when static branches, remote configuration, and manual
tuning are too rigid.

The application still owns execution. Flaggo owns the versioned decision
contract, approved authority, deterministic evaluation, policy, durable audit,
and exposure attribution around selected runtime variables.

## Current product slice

The current Phase 3 contract uses authenticated bundle approval:

```text
definition + initial authority candidate
  -> approval
  -> governed state
  -> deterministic runtime decision
  -> policy + durable audit
  -> confirmed exposure
  -> attributed outcome
```

Current authority is either an approved fixed value or an approved numeric
rule. Proposal generation, learned evidence, experiments, rollouts, and other
proposal-managed workflows remain future Phase 4 work.

The first scenario governs `tetris.dropInterval`, returning an approved
`750ms` or `850ms` value from live game inputs with an `800ms` fallback.

## Start here

- [Documentation home](docs/README.md) - product explanation, sources of truth,
  and reading paths.
- [Manifesto](docs/MANIFESTO.md) - project purpose and implementation
  principles.
- [Project roadmap](https://github.com/users/hahahahahaiyiwen/projects/3) -
  authoritative phase status and issue contracts.
- [Contributing](CONTRIBUTING.md) - development setup and repository rules.

## Quickstart

Install the contract-validation dependency and run the repository gate:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

Start the fixture-backed local API:

```powershell
python tools\dev.py serve
```

Or use Docker:

```powershell
docker compose up --build mock-api
```

List available fixtures at `http://127.0.0.1:8080/_fixtures`. No cloud account
is required.

## Repository layout

```text
apps/          Runnable services and user interfaces
modules/       Business capabilities and module-owned ports
packages/      Reusable SDK and contract packages
contracts/     OpenAPI, JSON Schema, fixtures, and conformance
tests/         Cross-module and end-to-end verification
examples/      Small integrations and showcase links
deploy/        Container and deployment assets
tools/         Repository development commands
docs/          Product, architecture, roadmap, and component designs
```

Flaggo is early-stage. Prefer the smallest complete end-to-end behavior over
speculative platform breadth.
