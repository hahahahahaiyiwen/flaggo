# Phase 3 implementation scope

## Purpose

Phase 3 establishes the executable v3 baseline delivered by #49. It separates
behavior that is implemented now from contract data retained for future
capabilities and from work that remains outside this phase.

The target architecture is broader than the Phase 3 implementation. A
`DecisionContract` may describe authored or learning intent that the current
runtime does not execute. Contract readiness therefore means that the required
default executable is durable and active; it does not mean that every optional
declaration has an operational producer.

## Implemented and executable

Phase 3 implements this synchronous path:

```text
DecisionContract
  -> validate and accept immutable contract version
  -> generate and activate required default executable
  -> optionally compile and activate deterministic authored expressions

exact contract name + contractDigest + complete RuntimeInput
  -> resolve ActiveExecutable in authenticated tenant/application/environment scope
  -> bounded deterministic evaluation
  -> validate and return RuntimeDecision
```

The executable boundary includes:

- Management API v3 validation, idempotent deployment, current lookup,
  history, and exact-version reads;
- Runtime API v3 evaluation for one exact contract name and digest;
- Contract Service and Decision Service as separate composition roots;
- immutable Contract Store and Executable Store records in one configured
  SQLite database;
- automatic default-executable generation and activation from
  `result.default`;
- bounded `flaggo.cel/v1` compilation for deterministic authored predicates
  and result expressions;
- Candidate, Active, and Inactive executable lifecycle state keyed by
  contract digest;
- strict JSON, correlation, Problem Details, and service health behavior;
- generated TypeScript wire types and validators with hand-written management
  and runtime clients;
- manifest-driven contract deployment separated from build and service
  startup; and
- real-host Tetris and Adaptive Worker examples.

Runtime evaluation is stateless. It does not persist an evaluation session,
idempotency record, decision record, confirmation, or exposure record.

## Accepted contract data without an operational producer

Some v3 fields preserve the intended architecture boundary without making the
corresponding future subsystem operational in Phase 3.

### Natural-language authored conditions

Contract Acceptance validates and stores natural-language authored conditions
as source material. Phase 3 does not generate a deterministic executable from
them.

If an `authoredExecutable` contains any natural-language condition, Phase 3
does not compile or activate that authored executable. The contract remains
runtime-ready through its required default executable. Runtime never evaluates
natural language.

This behavior preserves the distinction between accepted authored intent and
active executable authority. A later generator may produce a deterministic
candidate through the same candidate-validation-activation boundary.

### Learning declarations

Contract Acceptance validates and stores `learning` declarations and moves the
management current pointer to the newly accepted ready digest. Phase 3 has no
learning scheduler, evidence reader, analysis worker, or evidence-generated
candidate producer.

`mode: auto-activation` defines how a future valid, current
evidence-generated candidate crosses the activation boundary. It does not
start learning by itself and does not make Decision Service search for a
newest executable.

### Current exposure context

The runtime wire model and SDK accept optional `currentExposure` context.
Phase 3 validates and transports that context but does not persist it, emit
exposure telemetry, or use it as an evaluator input.

## Outside Phase 3

The following capabilities remain deferred:

- generation of deterministic candidates from natural-language conditions;
- SDK exposure creation or OpenTelemetry exposure emission;
- OpenTelemetry evidence ingestion and correlation;
- an Evidence Store;
- a learning scheduler or asynchronous analysis worker;
- evidence-generated candidate production;
- experiments, rollout governance, or manual executable approval;
- SDK-synthesized outage fallback decisions;
- durable runtime request, decision, confirmation, or exposure records;
- compatibility routes or adapters for the retired v1/v2 architecture; and
- a production container or orchestrator topology.

Applications may implement their own failure behavior. For example, the
Tetris application uses local gravity before its first successful Flaggo
decision and retains the last successful interval after later failures. That
is application behavior, not SDK or Decision Service fallback.

## Phase boundary invariants

1. Every accepted-ready contract digest has a durable active default
   executable.
2. Optional declarations do not gain runtime authority merely by being
   accepted.
3. Activation is the only operation that grants executable runtime authority.
4. Runtime resolves only the activation for the exact supplied digest.
5. A newer digest does not deactivate or reinterpret an older digest.
6. Evidence, learning, and exposure work never runs in the synchronous
   decision path.
7. Returning a decision does not establish that the application applied it.

Future phases extend candidate production and evidence flow behind these
boundaries rather than changing exact-version runtime authority.
