# Domain and store modules

Flaggo uses explicit service and store boundaries:

```text
contract
expression
decision
contract-store
executable-store
evidence-store
raw-otlp-inbox
hosting
service-observability
contract-application
decision-http
otlp-http
analysis-domain
analysis-workspace
analysis-agent
analysis-application
analysis-execution
core-conformance
```

The semantic owner and allowed access for each store are defined in
[Application boundaries and lifecycle](../docs/design/architecture/APP_BOUNDARIES.md#durable-state-ownership).

Each module owns one cohesive domain, store, or hosting boundary. Modules do
not depend on an app composition root or on the legacy
`packages/shared-contracts` project.

- Contract owns authored contract/executable models and scoped v4 wire models, bounded value-schema
  validation, canonicalization, and semantic digests.
- Expression owns the bounded `flaggo.cel/v1` compiler and evaluator.
- Decision owns one stateless exact-version runtime evaluation.
- Contract Store persists immutable accepted versions and management
  projections.
- Executable Store persists immutable executable artifacts and atomically owns
  scoped Candidate, Active, and Inactive lifecycle state.
- Evidence Store is a Rust storage-neutral contract with a SQLx/SQLite adapter.
  It atomically persists selected query-ready telemetry observations,
  materialization provenance, diagnostics, the current catalog cache, one
  global forward checkpoint, and logical-source conflicts for asynchronous
  analysis.
- Raw OTLP Inbox persists complete validated OTLP export requests as a bounded,
  replayable work log without depending on its SQLx/SQLite adapter contract.
- Hosting owns reusable Rust HTTP, correlation, Problem Details, health, and
  OpenTelemetry request instrumentation without domain routes.
- Service Observability owns the shared Rust Resource, OTLP exporter,
  structured-console, trace-propagation, and bounded-shutdown composition.
- Analysis Application owns orchestration, the scoped Contract Service client,
  evidence/tool behavior and injected execution capabilities. It depends on
  neither SQLite nor local execution/workspace implementations.
- Analysis Workspace owns scoped claims, immutable artifacts, conditional
  publication and durable execution receipts. Its default `local` feature
  implements those contracts with filesystem state and OS lock exclusion.
- Analysis Execution defines the path-free execution provider; its `local`
  feature supplies trusted local Python execution, not a hosted sandbox.
- Core Conformance provides authority, evidence and workflow outcome scenarios.

The former Audit, Policy, Reasoning, Registry, Evidence, and State modules were
removed by #49. Do not reintroduce forwarding projects, namespace aliases,
dual registrations, migration readers, or compatibility fixtures.

Contract Store and Executable Store's default SQLite adapters use direct SQLx/SQLite
dependencies and store-owned SQL; their portable contracts disable default features. They share one
configured local database but no custom storage-infrastructure package.
Raw OTLP Inbox and Evidence Store use direct SQLx/SQLite adapters and own
separate schema components in that database; neither exposes its tables as a
cross-module contract.

The `-service` suffix is reserved for executable projects under `apps`.

See [the extraction compatibility record](../docs/design/architecture/CORE_EXTRACTION.md)
for capability revisions, explicit local migrations and adapter checks.
