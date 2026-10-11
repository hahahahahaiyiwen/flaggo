# Flaggo documentation

Flaggo lets applications delegate selected runtime variables to explicit,
versioned `DecisionContract` resources and immutable `DecisionExecutable`
artifacts.

The implemented stack covers contract acceptance, default and deterministic
authored-executable generation, atomic activation, stateless runtime
evaluation, direct OTLP ingestion, a bounded Raw OTLP Inbox, and forward-only
Evidence Materialization, asynchronous bounded evidence analysis, immutable
Candidate admission, and auto-activation. Applications own exposure/outcome
correlation; analysis joins the declared evidence rather than relying on an
ingestion-time correlation guarantee.

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

decision receipt + application telemetry
  -> Raw OTLP Inbox
  -> Evidence Store
  -> Async Analysis
  -> CandidateExecutable
  -> Contract Service validation and activation
```

Activation is the only transition that grants runtime authority. The initial
learning declaration uses `mode: auto-activation` to request activation of an
eligible validated evidence-generated Candidate. Async Analysis cannot
write runtime authority directly; its candidate returns through Contract
Service validation and activation. Runtime never selects a contract or
executable by generation recency.

## Current state

The v3 management and runtime contracts are implemented by Contract Service,
Decision Service, reusable modules, and the TypeScript SDK. OTel Ingestion and
Evidence Materializer implement the telemetry-to-query-ready-evidence
foundation. Management addresses a logical decision by name and immutable
versions by `contractDigest`; runtime resolves only the executable active for
the exact requested digest and authenticated tenant/application/environment
scope.

The pre-v3 implementation has been removed. The
[Tetris playground](../examples/tetris-playground/README.md) exercises the
learning loop with real local services and a standard local OTel backend.
Its credential-free benchmark uses an example-owned deterministic agent;
live learning uses Copilot. Neither admission nor the greedy-player benchmark
proves production safety or human enjoyment.

[Project #3](https://github.com/users/hahahahahaiyiwen/projects/3), native issue
dependencies, and self-contained issue contracts remain the authoritative
roadmap and status source. This page is orientation, not a second roadmap.

## Sources of truth

| Question | Source |
| --- | --- |
| Why does Flaggo exist? | [Manifesto](MANIFESTO.md) |
| What is the canonical lifecycle and who owns each app or store boundary? | [Application boundaries and lifecycle](design/architecture/APP_BOUNDARIES.md) |
| What does each runnable composition root own? | [`apps/`](../apps/README.md) |
| What did the Phase 3 management/runtime milestone contain? | [Phase 3 implementation scope](design/PHASE_3.md) |
| How do clients, services, workers, and stores fit together? | [Architecture overview](design/architecture/OVERVIEW.md) |
| How do authoring clients deploy and inspect contracts? | [Contract clients and Contract Service](design/architecture/CONTRACT_SERVICE.md) |
| How do applications and the SDK call runtime? | [Runtime client and Decision Service](design/architecture/RUNTIME.md) |
| How do OTLP ingestion, materialization, evidence, and analysis relate? | [Evidence and learning](design/architecture/EVIDENCE.md) |
| What are the contract and executable primitives? | [Decision contracts and executables](design/contracts/CONTRACTS.md) |
| How does the lifecycle progress? | [Decision contract lifecycle](design/contracts/LIFECYCLE.md) |
| How does exact-version stateless evaluation work? | [Runtime evaluation model](design/contracts/RUNTIME_EVALUATION.md) |
| What are the executable wire contracts? | [`contracts/`](../contracts/README.md) |

OpenAPI documents, JSON Schemas, OTel profiles, fixtures, and conformance tests
are authoritative for supported wire and telemetry behavior. Architecture
documents define ownership and invariants.
