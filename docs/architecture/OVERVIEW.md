# Flaggo architecture overview

## Purpose

Flaggo is a closed-loop decisioning system with two online services, one
telemetry ingress, an optional async worker family, and three durable stores.
This document defines the shared vocabulary and ownership model.

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
  -> Evidence Store (decision and exposure records)

OTel Ingestion
  -> Evidence Store (observations)

Async Analysis Pipeline
  <- Contract Store
  <- Evidence Store
  <- State Store
  -> candidate -> Contract Service approval
```

Arrows show allowed ownership paths, not direct database access requirements.
Adapters may expose ports or service APIs while preserving the same boundary.

## Core vocabulary

| Concept | Meaning | Owner |
| --- | --- | --- |
| Decision key | Stable developer-facing name for one decision family. | Decision definition |
| Decision definition | Versioned semantic contract: targets, inputs, output, constraints, fallback, and authority workflow. | Contract Service / Contract Store |
| Runtime identity | Exact `{ definitionId, revision, contractDigest }` expected by a request. | Contract Service |
| Decision constraints | Deterministic limits that validate candidates or runtime results. | Declared in definitions; evaluated by owning service |
| Authority candidate | Bounded behavior awaiting authenticated approval. | Bundle or Async Analysis Pipeline |
| Decision state | Immutable activated `active-value` or `numeric-rule` authority. | State Store |
| Runtime inputs | Typed values resolved for one execution from request or authorized evidence sources. | Decision Service input resolution from application request / Evidence Store |
| Evidence | Durable observed or derived knowledge. | Evidence Store |
| Decision record | Reconstructable record of one server result. | Decision Service -> Evidence Store |
| Exposure | Confirmation that the application applied or rendered a result. | Decision Service -> Evidence Store |
| Outcome | Observation attributed to a confirmed exposure. | Evidence Store |

`RuntimeDecisionResult` records what one request received. It is not authority.

## First-class boundaries

### Contract Service

The control-plane service validates and stores definitions, records exact
approvals, orchestrates authority activation, and returns ready bindings. It
owns Contract Store and is the only service allowed to activate state.

### Decision Service

The online data-plane service resolves ready contracts and active state,
executes bounded authority, evaluates decision constraints, selects governed
fallback, appends durable decision records, and confirms exposure.

### OTel Ingestion

The server ingress accepts supported OTLP data from the application's existing
telemetry pipeline and appends normalized observations to Evidence Store.

### Async Analysis Pipeline

Offline workers may read contracts, evidence, and current state to produce
bounded candidates. They cannot approve candidates or write active state.

### Durable stores

- Contract Store: definitions, revisions, approvals, and readiness.
- State Store: stable authority heads, immutable states, CAS, replay, lineage.
- Evidence Store: observations, views, decisions, exposures, and outcomes.

## Embedded capabilities

Policy is not a component. Decision constraints are contract data and service
behavior.

Audit is not a component. Durable, reconstructable records are required outputs
of Contract Service lifecycle changes and Decision Service results.

Explanation is not a component. It is a deterministic projection of stored
facts.

The operator console is not a server component. It is a future client of
management, query, state, and evidence APIs.

The former reasoning boundary is split: bounded online execution is inside
Decision Service; async candidate production belongs to Async Analysis
Pipeline.

## Control-plane flow

```text
decision definition + initial authority candidate
  -> Contract Service canonical validation
  -> authenticated exact-snapshot approval
  -> captured stable-head baseline
  -> State Store compare-and-swap
  -> ready registration binding
```

The client cannot supply trusted approval, activation, state, or strategy
identities.

## Data-plane flow

```text
exact runtime identity + target + live inputs
  -> ready Contract Store projection
  -> compatible State Store authority
  -> active value or numeric-rule execution
  -> decision-constraint evaluation
  -> governed result or fallback
  -> durable Evidence Store decision record
  -> RuntimeDecisionResult
```

Runtime cannot create authority.

## Telemetry and closed-loop flow

```text
application-owned OTel export
  -> OTel Ingestion
  -> Evidence Store observations

decision result
  -> application applies value
  -> explicit exposure confirmation
  -> Evidence Store exposure
  -> exposure-linked outcomes

contracts + evidence + current state
  -> Async Analysis Pipeline candidate
  -> Contract Service governance
```

## Identity and reuse

The runtime identity is the complete definition tuple. The stable authority
address is application + environment + decision key + control target.

Observations and evidence views may be reused when their immutable semantics
match. Decision state is never silently reused across exact runtime identities.
Decision, exposure, and outcome records remain bound to the identity used.

## Failure and fallback

Missing, conflicting, retired, or non-ready identities are explicit contract or
readiness errors. Corrupt stores fail closed.

A valid registered request may return governed server fallback. An SDK
availability fallback is application-local and has no server decision,
constraint, durable-record, or exposure identity.

## Current implementation mapping

Current assemblies remain modular implementation libraries. Logical ownership
does not require one assembly per boundary:

| Current implementation | Target ownership |
| --- | --- |
| Control-plane host and Registry module | Contract Service / Contract Store |
| Data-plane host and Decisioning module | Decision Service |
| Policy module | Internal constraint evaluation in Contract and Decision Services |
| Audit module | Evidence Store append adapter used by Decision Service |
| State module | State Store |
| Evidence module | Evidence Store and projection ports |
| Reasoning module | Decision Service executor plus future Async Analysis ports |

Issue #40 owns executable migration. Issue #41 verifies the integrated Phase 3
path.

## Delivery impact map

| Follow-up | Required outcome |
| --- | --- |
| #40 | Compose Contract Service and Decision Service against the three stores; migrate `policy`/`InlinePolicy`/`PolicyEvaluationResult` to definition-owned `constraints`/`DecisionConstraints`/`ConstraintEvaluationResult`; remove `PolicyReference`; migrate `auditId`/`AuditRecord`/`IAuditSink` to `decisionRecordId`/`DecisionRecord`/Evidence Store append; preserve approval, CAS, replay, readiness, fallback, and exposure behavior. |
| #41 | Run the real Tetris path using a trusted manifest publisher plus key-based runtime SDK; verify the seven logical boundaries, deterministic constraints without standalone Policy, durable records without standalone Audit, application-owned OTel export, and preserved authority/fallback/exposure invariants. |

The operative issue contracts for #40 and #41 use these target names. They do
not preserve compatibility aliases for the executable migration.

## Related documents

- [Decision definition](DECISION_DEFINITION.md)
- [Evidence](EVIDENCE.md)
- [Authority](AUTHORITY.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Design index](../design/README.md)
- [Tetris scenario](../scenarios/TETRIS.md)
