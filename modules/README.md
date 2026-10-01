# Domain and store modules

Phase 3 uses explicit service and store boundaries:

```text
contract
expression
decision
contract-store
executable-store
evidence-store
raw-otlp-inbox
hosting
```

Each module owns one cohesive domain, store, or hosting boundary. Modules do
not depend on an app composition root or on the legacy
`packages/shared-contracts` project.

- Contract owns v3 contract/executable/runtime models, bounded value-schema
  validation, canonicalization, and semantic digests.
- Expression owns the bounded `flaggo.cel/v1` compiler and evaluator.
- Decision owns one stateless exact-version runtime evaluation.
- Contract Store persists immutable accepted versions and management
  projections.
- Executable Store persists immutable executable artifacts and atomically owns
  scoped Candidate, Active, and Inactive lifecycle state.
- Evidence Store persists selected, query-ready telemetry observations and
  materialization provenance for asynchronous analysis.
- Raw OTLP Inbox persists complete validated OTLP export requests as a bounded,
  replayable work log without depending on its SQLx/SQLite adapter contract.
- Hosting owns reusable strict HTTP, authentication, correlation, Problem
  Details, and health mechanics without domain routes.

The former Audit, Policy, Reasoning, Registry, Evidence, and State modules were
removed by #49. Do not reintroduce forwarding projects, namespace aliases,
dual registrations, migration readers, or compatibility fixtures.

Contract Store and Executable Store use direct
`Microsoft.Data.Sqlite` dependencies and store-owned SQL. They share one
configured local database but no custom storage-infrastructure package.

Future Async Analysis Pipeline modules require a separately accepted design.

The `-service` suffix is reserved for executable projects under `apps`.
