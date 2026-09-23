# Flaggo documentation

Flaggo lets applications delegate selected runtime variables to explicit,
versioned decision definitions with approved authority, deterministic
constraints, durable records, and exposure-linked outcomes.

The application owns execution and telemetry export. Flaggo owns server-side
registration, authority, runtime evaluation, ingestion, and durable stores.

## Product model

```text
current bundle-approved path:
decision definition + initial authority candidate
  -> Contract Service approval
  -> State Store activation
  -> Decision Service execution
  -> decision constraints + durable decision record
  -> confirmed exposure
  -> attributed outcome

future proposal-managed extension:
contracts + Evidence Store + current state
  -> Async Analysis Pipeline candidate
  -> the same Contract Service approval and activation boundary
```

Phase 3 does not claim learned proposal generation, experiments, rollouts, or
request-time AI.

## Sources of truth

| Question | Source |
| --- | --- |
| Why does Flaggo exist? | [Manifesto](MANIFESTO.md) |
| How do services, stores, clients, and workers fit together? | [Architecture overview](architecture/OVERVIEW.md) |
| What does a decision definition own? | [Decision definition](architecture/DECISION_DEFINITION.md) |
| How do observations, evidence, decisions, exposures, and outcomes differ? | [Evidence](architecture/EVIDENCE.md) |
| How does a candidate become active authority? | [Authority](architecture/AUTHORITY.md) |
| How does one online request produce a result? | [Runtime execution](architecture/RUNTIME_EXECUTION.md) |
| What should the first product slice demonstrate? | [Tetris scenario](scenarios/TETRIS.md) |
| Which boundary owns detailed behavior? | [Design index](design/README.md) |
| What is implemented next? | [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3) |
| What are the executable wire contracts? | [Contracts](../contracts/README.md) |

Executable OpenAPI, schemas, fixtures, and conformance tests remain
authoritative for current wire behavior. Architecture documents define target
ownership and guide issue-scoped migrations.

## Reading paths

### New teammate

1. [Manifesto](MANIFESTO.md)
2. [Architecture overview](architecture/OVERVIEW.md)
3. [Tetris scenario](scenarios/TETRIS.md)
4. [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3)

### Server contributor

1. Start from the accepted issue.
2. Read the relevant service or store in the [design index](design/README.md).
3. Read [Authority](architecture/AUTHORITY.md) and
   [Runtime execution](architecture/RUNTIME_EXECUTION.md).
4. Check [executable contracts](../contracts/README.md).

### Contract or SDK contributor

1. Read [Decision definition](architecture/DECISION_DEFINITION.md).
2. Read [shared contracts](design/shared-contracts/README.md).
3. Read [client library](design/client-library/README.md).
4. Read [Contract Service](design/contract-service/README.md) and
   [Decision Service](design/decision-service/README.md).

## Documentation rules

- Keep one primary question per canonical document.
- Use one canonical name for each concept.
- Describe logical services and stores independently from current assembly
  names.
- Keep Project #3 and issue contracts authoritative for roadmap status.
- Mark deferred behavior explicitly.
- Remove obsolete paths instead of preserving compatibility copies.
