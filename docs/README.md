# Flaggo documentation

Flaggo lets applications delegate selected runtime variables to explicit,
versioned `DecisionContract` resources and immutable `DecisionExecutable`
artifacts.

In Phase 3, the SDK constructs complete runtime input and the application
decides how to apply returned results. Flaggo implements contract acceptance,
default and deterministic authored-executable generation, atomic activation,
and stateless runtime evaluation. Exposure telemetry, evidence ingestion and
correlation, and asynchronous evidence-based generation remain future work.

## Product model

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
never selects a contract or executable by generation recency.

## Current state

The v3 management and runtime contracts are implemented by Contract Service,
Decision Service, their shared modules, and the TypeScript SDK. Management
addresses a logical decision by name and immutable versions by
`contractDigest`. Runtime requests one exact name/digest pair and resolves the
executable active for that digest in the authenticated application/environment
scope.

The pre-v3 implementation has been removed. Evidence ingestion and
asynchronous evidence-based generation remain future implementation work.

[Project #3](https://github.com/users/hahahahahaiyiwen/projects/3), native issue
dependencies, and self-contained issue contracts remain the authoritative
roadmap and status source. This page is orientation, not a second roadmap.

## Sources of truth

| Question | Source |
| --- | --- |
| Why does Flaggo exist? | [Manifesto](MANIFESTO.md) |
| What is implemented in Phase 3 and what remains deferred? | [Phase 3 implementation scope](design/PHASE_3.md) |
| How do clients, services, workers, and stores fit together? | [Architecture overview](design/architecture/OVERVIEW.md) |
| How do authoring clients deploy and inspect contracts? | [Contract clients and Contract Service](design/architecture/CONTRACT_SERVICE.md) |
| How do applications and the SDK call runtime? | [Runtime client and Decision Service](design/architecture/RUNTIME.md) |
| What are the contract and executable primitives? | [Decision contracts and executables](design/contracts/CONTRACTS.md) |
| How does the lifecycle progress? | [Decision contract lifecycle](design/contracts/LIFECYCLE.md) |
| How does exact-version stateless evaluation work? | [Runtime evaluation model](design/contracts/RUNTIME_EVALUATION.md) |
| What are the executable wire contracts? | [`contracts/`](../contracts/README.md) |

OpenAPI documents, JSON Schemas, OTel profiles, fixtures, and conformance tests
are authoritative for supported wire and telemetry behavior. Architecture
documents define ownership and invariants.
