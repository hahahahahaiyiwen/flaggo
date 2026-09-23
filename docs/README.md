# Flaggo documentation

Flaggo is a policy-first decisioning control plane with a runtime decision
provider. It lets application code delegate selected runtime variables to an
explicit, versioned, policy-gated contract instead of burying those choices in
scattered branches, dashboards, and manual tuning.

The application still owns execution. Flaggo owns the contract, authority,
runtime evaluation, policy, audit, and attribution around the delegated
decision.

## Product model

Feature flags and remote configuration externalize values. Flaggo adds a
governed decision loop around variables whose safe value depends on runtime
context, declared constraints, and eventually accumulated outcomes.

The current contract boundary is deliberately smaller than the long-term
vision:

```text
current bundle-approved path:
definition + initial authority candidate
  -> authenticated approval
  -> governed state
  -> deterministic runtime evaluation
  -> policy + durable audit
  -> decision result
  -> confirmed exposure
  -> attributed outcome

future proposal-managed extension:
definition + evidence + outcomes + objectives
  -> bounded proposal
  -> governance
  -> the same governed-state activation boundary
```

Phase 3 does not claim learned evidence, proposal generation, experiments,
rollouts, or request-time AI. Its Tetris path executes an explicitly approved
`active-value` or `numeric-rule` authority. Phase 4 remains a high-level
extension until its proposal and governance contracts are accepted.

## Current state

The repository contains the accepted Phase 1 executable contracts, the
TypeScript SDK and .NET service boundaries developed against them, and a
cloud-free adaptive-worker integration path. The Phase 3 roadmap is
re-baselined around authenticated bundle approval and the shared activation
boundary; the implementation is not considered complete until the state,
bundle, durable-audit, and Tetris integration follow-ups converge.

[Project #3](https://github.com/users/hahahahahaiyiwen/projects/3), native issue
dependencies, and self-contained issue contracts are the authoritative roadmap
and status source. This page is orientation, not a second roadmap.

## Sources of truth

| Question | Source |
| --- | --- |
| Why does Flaggo exist, and how should contributors build it? | [Manifesto](MANIFESTO.md) |
| How do the concepts and system boundaries fit together? | [Architecture overview](architecture/OVERVIEW.md) |
| What does a decision definition own? | [Decision definition](architecture/DECISION_DEFINITION.md) |
| How do context, signals, evidence, exposure, and outcomes differ? | [Evidence](architecture/EVIDENCE.md) |
| How does declared or proposed behavior become runtime authority? | [Authority](architecture/AUTHORITY.md) |
| How does the data plane produce one decision result? | [Runtime execution](architecture/RUNTIME_EXECUTION.md) |
| What exact behavior should the first product slice demonstrate? | [Tetris scenario](scenarios/TETRIS.md) |
| What is implemented next and in what order? | [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3) and its issue contracts |
| Which component owns each detailed responsibility? | [Component design index](design/README.md) |
| What are the exact executable wire contracts? | [Contracts](../contracts/README.md) |
| How is the repository organized and changed? | [Contributing](../CONTRIBUTING.md) |

The OpenAPI documents, JSON Schemas, fixtures, and conformance tests under
[`contracts/`](../contracts/README.md) are authoritative for executable wire
behavior. Architecture documents explain intent and boundaries; they do not
override executable contracts.

## Reading paths

### New teammate

1. Read the [manifesto](MANIFESTO.md) for the project purpose and implementation
   principles.
2. Read the [architecture overview](architecture/OVERVIEW.md) for the shared
   vocabulary and system map.
3. Walk through the [Tetris scenario](scenarios/TETRIS.md) for the concrete
   `750ms` / `850ms` / `800ms` behavior.
4. Use [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3) and
   the linked issue contracts to understand current phase boundaries.

### Implementer

1. Start with the accepted issue for the relevant phase on
   [Project #3](https://github.com/users/hahahahahaiyiwen/projects/3).
2. Follow the [component design index](design/README.md) to the owning module.
3. Check the [executable contracts](../contracts/README.md) before changing a
   wire shape.
4. Follow the repository rules in [CONTRIBUTING.md](../CONTRIBUTING.md).

### Contract or SDK contributor

1. Read the [Phase 1 API contract proposal](design/API_CONTRACT_PROPOSAL.md).
2. Read the [shared contracts design](design/shared-contracts/README.md).
3. Inspect the OpenAPI, schema, fixtures, and conformance gate under
   [`contracts/`](../contracts/README.md).
4. Use the [client-library design](design/client-library/README.md) or
   [Decision API design](design/decision-api/README.md) for the owning
   implementation boundary.

### Authority or runtime contributor

1. Read [Authority](architecture/AUTHORITY.md).
2. Read [Runtime execution](architecture/RUNTIME_EXECUTION.md).
3. Follow the state, policy, reasoning, audit, registry, and Decision API links
   from the [component design index](design/README.md).

## Documentation rules

- Keep one primary question per canonical document.
- Keep Project #3, native dependencies, and issue contracts authoritative for
  roadmap status.
- Keep exact wire behavior in executable contracts and their owning component
  designs.
- Mark future behavior explicitly; do not describe deferred proposal,
  experiment, rollout, override, or rollback contracts as current.
- Remove obsolete documentation paths instead of maintaining compatibility
  copies.
