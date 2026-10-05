# Flaggo design index

The current target model is:

```text
DecisionContract
  -> Contract Acceptance
  -> Executable Generation
  -> Activation
  -> ActiveExecutable

ActiveExecutable + complete RuntimeInput
  -> stateless deterministic RuntimeDecision

RuntimeDecision receipt + application telemetry
  -> OTLP ingestion
  -> forward-only evidence materialization
  -> planned asynchronous analysis
  -> immutable candidate proposal
  -> Contract Service validation and activation
```

The application and SDK own complete input construction and the transition
from a returned decision to application behavior. The current SDK reports
decision receipt, while ordinary application telemetry supplies surrounding
activity. Evidence flows through OpenTelemetry into asynchronous processing; it
is not a runtime operand.

## Implementation phase

[Phase 3 implementation scope](PHASE_3.md) records the earlier management and
runtime milestone. The current stack also implements direct OTLP ingestion, a
bounded Raw OTLP Inbox, forward-only Evidence Materialization, and durable
Evidence Store observations. Async Analysis, evidence correlation, and learned
candidate generation remain planned. The GitHub Project and native issue graph
are the roadmap authority.

## Architecture

| Question | Current design |
| --- | --- |
| What is the canonical end-to-end lifecycle and who owns each boundary? | [Application boundaries and lifecycle](architecture/APP_BOUNDARIES.md) |
| How do clients, services, workers, and stores fit together? | [Architecture overview](architecture/OVERVIEW.md) |
| What establishes accepted and active authority? | [Decision authority](architecture/AUTHORITY.md) |
| How do authoring clients and Contract Service manage versions and activation? | [Contract clients and Contract Service](architecture/CONTRACT_SERVICE.md) |
| How do applications, SDKs, and Decision Service evaluate an exact version? | [Runtime client and Decision Service](architecture/RUNTIME.md) |
| How do exposure, outcomes, and learning relate? | [Evidence and learning](architecture/EVIDENCE.md) |
| How are Flaggo services observed without conflating operations with application evidence? | [Internal service observability](architecture/OBSERVABILITY.md) |

These documents define logical ownership. An implementation may co-locate
services or stores without changing those boundaries.

## Contract semantics and wire models

| Concern | Current design |
| --- | --- |
| `DecisionContract` and `DecisionExecutable` shapes and identities | [Decision contracts and executables](contracts/CONTRACTS.md) |
| Acceptance, generation, activation, and continuous learning | [Decision contract lifecycle](contracts/LIFECYCLE.md) |
| Complete SDK input, exact-version resolution, evaluation, retries, and failures | [Runtime evaluation model](contracts/RUNTIME_EVALUATION.md) |
| Executable API, JSON Schema, fixtures, and conformance | [`contracts/`](../../contracts/README.md) |

The v3 OpenAPI and JSON Schemas are authoritative for wire behavior. Design
documents explain the target ownership and invariants behind those contracts.

## Implemented app boundaries

| App | Current responsibility |
| --- | --- |
| Contract Service | Accept immutable named versions, generate default and authored executables, and own candidate validation and activation |
| Decision Service | Resolve the executable active for an exact requested digest and return a bounded decision without semantic request persistence |
| OTel Ingestion | Validate OTLP transport and append complete requests to the bounded Raw OTLP Inbox before acknowledgement |
| Evidence Materializer | Read retained inbox batches forward and persist query-ready observations, provenance, diagnostics, conflicts, catalog cache, and checkpoint |

The app-local operational contracts are indexed in
[`apps/README.md`](../../apps/README.md). Async Analysis can extend the existing
candidate-validation-and-activation boundary without changing runtime identity
or writing runtime authority directly.

## Archived designs

The documents under [`archived/`](archived/) describe the earlier
definition-bundle, target/State Store, durable-decision, explicit-confirmation,
and synchronous evidence-input architecture. They are retained for historical
implementation context and are not normative for the v3 target model.
