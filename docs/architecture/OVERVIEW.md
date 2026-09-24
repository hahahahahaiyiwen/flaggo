# Flaggo architecture overview

## Purpose

Flaggo is a closed-loop decisioning system with two online services, one
telemetry ingress, an optional async worker family, and three durable stores.
This document defines logical ownership, independently of assembly count.

## System at a glance

```text
application
  Client SDK --------------------------> Decision Service
  application OTel pipeline ----------> OTel Ingestion

trusted deployment/control-plane client
  canonical manifest -----------------> Contract Service

Contract Service
  -> Contract Store
  -> approved activation -> State Store

Decision Service
  <- Contract Store
  <- State Store
  <- Evidence Store (materialized inputs)
  -> Evidence Store (decision and exposure records)

OTel Ingestion
  -> Evidence Store (observations and validated attributed outcomes)

Async Analysis Pipeline
  <- Contract Store
  <- Evidence Store
  <- State Store
  -> candidate -> Contract Service approval
```

Arrows show allowed ownership paths, not direct database access requirements.
Adapters may expose ports or service APIs while preserving the boundary.

The current manifest-first slice publishes definitions separately from runtime
client initialization and consumes existing approved state. Examples provision
that state through trusted local bootstrap. #49 owns executable server
alignment; #40 connects manifest initial authority to activation-ready receipts.
Async candidate production remains future work, not request-time execution.

## Core vocabulary

| Concept | Meaning | Owner |
| --- | --- | --- |
| Decision key | Stable developer-facing decision family. | Decision definition |
| Decision definition | Semantic contract for targets, inputs, result/default, evidence interpretations, intent, and constraints. | Contract Service / Contract Store |
| Runtime identity | Exact `{ definitionId, revision, contractDigest }` expected by a request. | Contract Service |
| Decision constraints | Deterministic limits that may narrow, never widen, authority. | Declared in definitions; evaluated by owning service |
| Authority candidate | Bounded behavior awaiting authenticated approval. | Trusted publisher or future Async Analysis Pipeline |
| Decision state | Immutable activated `active-value` or `numeric-rule` authority. | State Store |
| Runtime inputs | Typed values resolved from request or authorized materialized evidence. | Decision Service input resolution |
| Evidence | Received observations, materialized views, and reconstructable records. | Evidence Store |
| Decision record | Facts reconstructing one server result. | Decision Service -> Evidence Store |
| Exposure | Explicit confirmation of application use. | Decision Service -> Evidence Store |
| Outcome | Observation validated and attributed to a confirmed exposure. | OTel Ingestion -> Evidence Store |

A runtime decision result records what one request received. It is not
authority or proof of application use.

## First-class boundaries

- **Contract Service** owns definition validation, immutable publication,
  exact approval, and activation/readiness orchestration. It owns Contract
  Store and is the only service allowed to orchestrate state activation.
- **Decision Service** resolves contracts, targets, required inputs and active
  state; executes bounded authority; evaluates constraints; records the result;
  and confirms exposure.
- **OTel Ingestion** accepts supported application-owned OTLP data, validates
  bindings and attribution, and materializes evidence outside execution.
- **Async Analysis Pipeline** may produce bounded candidates from contracts,
  evidence and current state. It cannot approve or activate candidates.
- **Contract Store** retains definitions, revisions, approvals and readiness.
- **State Store** retains stable authority heads, immutable states, CAS,
  replay and lineage.
- **Evidence Store** owns observations, materialized views, decision records,
  exposures and outcomes.

Policy is not a component: decision constraints are contract data and service
behavior. Audit is not a component: durable, reconstructable records are
required outputs. Explanation is a deterministic projection of stored facts.
An operator console is a future client, not another server component.
Bounded online reasoning belongs inside Decision Service; candidate production
belongs to Async Analysis Pipeline.

## Control-plane flow

The target bundle-approved path, implemented by #40 after #49, is:

```text
definition + initial authority candidate
  -> Contract Service canonical validation
  -> authenticated exact-snapshot approval
  -> captured stable-head baseline
  -> State Store compare-and-swap
  -> activation-ready runtime binding
```

Current manifest v2 publication returns an approved-definition receipt, not
proof of this future activation. The client cannot supply trusted approval,
activation, state, or strategy identities. Publication and application
deployment remain independent; browser code contains no management credentials.

## Data-plane flow

```text
exact identity + runtime target/context + request inputs
  -> accepted Contract Store projection
  -> ordered target resolution
  -> required request/evidence inputs from one pinned generation
  -> compatible State Store authority
  -> active value or numeric-rule execution
  -> decision constraints and governed fallback
  -> durable Evidence Store decision record
  -> runtime decision result
```

The executor receives resolved typed values, not raw telemetry or evidence
snapshots. Runtime cannot create authority.

## Telemetry and closed-loop flow

```text
application OTel export -> OTel Ingestion -> materialized observations

decision result -> application applies value -> explicit confirmation
  -> confirmed exposure -> application emits an exposure-linked span/log
  -> OTel Ingestion validates binding and completed confirmation
  -> attributed outcome

contracts + evidence + current state
  -> future Async Analysis Pipeline candidate
  -> Contract Service approval and activation
```

Applications retain existing producers, providers, exporters and Collectors.
Flaggo does not create a parallel producer schema or export pipeline.

## Identity, targets, and failure

The stable authority address is application + environment + decision key +
control target. One head orders replacement across semantic revisions; every
activated state belongs to the exact approved runtime identity.

Definitions authorize target kinds and explicit ordered fallbacks. Runtime,
control and evidence targets are distinct; input provenance retains the actual
evidence target even when it is outside the selected state fallback chain.
Current materialized frames partition authenticated tenant/app/environment,
exact definition, binding and target. No cross-revision reuse is implicit.

Unknown, conflicting, retired or non-ready identities fail explicitly.
Corrupt stores fail closed. Required unusable input evidence is not a default
or fallback-eligible outage. An SDK availability fallback has no server
decision, constraint, durable-record or exposure identity.

## Current implementation and follow-ups

| Current implementation | Logical ownership |
| --- | --- |
| Control-plane host and Registry library | Contract Service / Contract Store |
| Data-plane host and Decisioning library | Decision Service |
| Data-plane OTLP adapter | OTel Ingestion |
| Policy library | Internal constraint evaluation |
| Audit library | Evidence Store append adapter used by Decision Service |
| State library | State Store and current confirmation lookup implementation |
| Evidence library | Materialization, input views and separate quality ports |

These are implementation libraries, not additional logical components.

| Follow-up | Required outcome |
| --- | --- |
| #49 | Compose the server services/stores, migrate Policy/Audit-named contracts to definition constraints and Evidence Store records, and consume the manifest/input/evidence boundary without compatibility aliases. |
| #40 | Add bundle-approved initial authority, activation and ready receipts on the aligned baseline. |
| #41 | Verify final integrated Tetris authority, constraint, durable-record, OTel and exposure behavior. |

## Related documents

- [Decision definition](DECISION_DEFINITION.md)
- [Evidence](EVIDENCE.md)
- [Authority](AUTHORITY.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Design index](../design/README.md)
- [Tetris scenario](../scenarios/TETRIS.md)
