# Flaggo documentation

Flaggo lets applications delegate selected runtime variables to explicit,
versioned decision definitions with approved authority, deterministic
constraints, durable records, and exposure-linked outcomes.

The application owns execution and telemetry export. Flaggo owns server-side
registration, authority, runtime evaluation, ingestion, and durable stores.

## Product model

```text
current manifest-first path:
JSON manifest
  -> explicit authenticated publication/approval
  -> generated catalog + exact approved receipt
  -> request-owned / materialized OTel inputs + existing governed state
  -> deterministic runtime evaluation
  -> decision constraints + durable decision record
  -> decision result
  -> confirmed exposure
  -> attributed outcome

future proposal-managed extension:
contracts + Evidence Store + current state
  -> Async Analysis Pipeline candidate
  -> the same Contract Service approval and activation boundary
```

Phase 3 does not claim learned evidence, proposal generation, experiments,
rollouts, or request-time AI. Its Tetris path executes an explicitly approved
`active-value` or `numeric-rule` authority. Phase 4 remains a high-level
extension until its proposal and governance contracts are accepted.

## Current state

The repository contains manifest v2 executable contracts, a typed key-based
TypeScript client, .NET runtime/input-materialization boundaries, and
cloud-free worker, Tetris, and real Collector integrations. Applications own
instrumentation and collection; Flaggo owns interpretation of declared evidence.
The Phase 3 roadmap is
re-baselined around authenticated bundle approval and the shared activation
boundary; the implementation is not considered complete until the state,
bundle-authority and Tetris re-baselining follow-ups converge. #49 owns executable
server-boundary and terminology alignment; #40 owns
manifest initial authority and activation-ready receipts; those are not
implemented by current approved-definition publication.

[Project #3](https://github.com/users/hahahahahaiyiwen/projects/3), native issue
dependencies, and self-contained issue contracts are the authoritative roadmap
and status source. This page is orientation, not a second roadmap.

## Sources of truth

| Question | Source |
| --- | --- |
| Why does Flaggo exist? | [Manifesto](MANIFESTO.md) |
| How do services, stores, clients, and workers fit together? | [Architecture overview](architecture/OVERVIEW.md) |
| What does a decision definition own? | [Decision definition](architecture/DECISION_DEFINITION.md) |
| How do request inputs, native telemetry, evidence, exposure, and outcomes differ? | [Evidence](architecture/EVIDENCE.md) |
| How does declared or proposed behavior become runtime authority? | [Authority](architecture/AUTHORITY.md) |
| How does the data plane produce one decision result? | [Runtime execution](architecture/RUNTIME_EXECUTION.md) |
| What exact behavior should the first product slice demonstrate? | [Tetris scenario](scenarios/TETRIS.md) |
| What is implemented next and in what order? | [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3) and its issue contracts |
| Which service or store owns each detailed responsibility? | [Design index](design/README.md) |
| What are the exact executable wire contracts? | [Contracts](../contracts/README.md) |
| How is the repository organized and changed? | [Contributing](../CONTRIBUTING.md) |

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
5. Check the [API contract baseline](design/API_CONTRACT_PROPOSAL.md) and
   [executable contracts](../contracts/README.md).

## Documentation rules

- Keep one primary question per canonical document.
- Keep Project #3, native dependencies, and issue contracts authoritative for
  roadmap status.
- Use one canonical name per concept and logical service/store ownership
  independently from current assembly names.
- Keep exact wire behavior in executable contracts and their owning boundary
  designs.
- Mark future behavior explicitly; do not describe deferred proposal,
  experiment, rollout, override, or rollback contracts as current.
- Remove obsolete documentation paths instead of maintaining compatibility
  copies.

Executable examples: [worker](../examples/adaptive-worker/README.md),
[Tetris](../examples/tetris-integration/README.md), and
[native OTel evidence](../examples/otel-evidence/README.md).
