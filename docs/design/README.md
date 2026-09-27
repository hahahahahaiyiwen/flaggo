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
```

The application and SDK own complete input construction and the transition
from a returned decision to an applied exposure. Evidence flows through
OpenTelemetry into asynchronous learning; it is not a runtime operand.

## Architecture

| Question | Current design |
| --- | --- |
| How do clients, services, workers, and stores fit together? | [Architecture overview](architecture/OVERVIEW.md) |
| What establishes accepted and active authority? | [Decision authority](architecture/AUTHORITY.md) |
| How do authoring clients and Contract Service manage versions and activation? | [Contract clients and Contract Service](architecture/CONTRACT_SERVICE.md) |
| How do applications, SDKs, and Decision Service evaluate an exact version? | [Runtime client and Decision Service](architecture/RUNTIME.md) |
| How do exposure, outcomes, and learning relate? | [Evidence and learning](architecture/EVIDENCE.md) |

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

## Implemented server boundary

Contract Service validates and accepts immutable named versions, persists
canonical identities, generates default and expression-authored executables,
activates them for the authenticated scope and exact digest, and exposes
current and historical Management API v3 resources.

Decision Service validates complete SDK input, resolves the executable active
for the requested exact version, evaluates its checked expressions, and
returns a RuntimeDecision without persisting request state. Evidence-based
generation can extend the existing candidate-validation-activation boundary
without changing runtime contract identity.

## Archived designs

The documents under [`archived/`](archived/) describe the earlier
definition-bundle, target/State Store, durable-decision, explicit-confirmation,
and synchronous evidence-input architecture. They are retained for historical
implementation context and are not normative for the v3 target model.
