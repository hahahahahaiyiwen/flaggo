# Contract Service design

## Purpose

Contract Service is the Flaggo control-plane service. It turns submitted
decision definitions into immutable registered contracts and turns approved
authority candidates into active decision state.

It owns the management API and the Contract Store boundary. It is the only
server service allowed to orchestrate authority activation.

This document separates target activation ownership from current publication:
manifest v2 currently publishes approved definitions and typed bindings.
#49 aligns executable services/stores; #40 adds bundle initial authority and
activation-converged receipts. Current local state bootstrap is trusted
tooling, not a second public manifest or a completed readiness contract.

## Responsibilities

Contract Service:

- validates and canonicalizes `DecisionDefinitionBundle` submissions;
- computes semantic digests and allocates definition lineages and revisions;
- stores active, deprecated, and retired definition revisions;
- creates and records authenticated approval requests for exact snapshots;
- validates initial or later authority candidates against the exact definition;
- captures the expected stable authority baseline during approval;
- activates approved candidates through the State Store compare-and-swap port;
- withholds registration readiness until every required activation succeeds;
- returns exact runtime bindings and ready receipts; and
- exposes definition and approval history needed for reconstruction.

It does not execute an online decision, ingest telemetry, generate an async
candidate, or let a caller provide trusted approval, activation, state, or
strategy identities.

## Contract Store

Contract Store is the durable boundary owned by Contract Service. It contains:

- decision keys, definition lineages, revisions, and contract digests;
- lifecycle state (`active | deprecated | retired`);
- canonical definition content and runtime projections;
- approval actors, comments, timestamps, and exact approved snapshots;
- deterministic proposal and activation identities allocated by the server;
- expected authority baselines and activation outcomes; and
- registration readiness and receipts.

Definition revisions are append-only semantic records. Metadata may evolve only
when it cannot change runtime meaning. Normal workflows retire history instead
of deleting it.

## Authority activation

```text
submitted definition + authority candidate
  -> canonical validation
  -> exact-snapshot approval
  -> captured stable-head baseline
  -> State Store compare-and-swap
  -> ready registration binding
```

Contract Service owns the workflow; State Store owns atomic state persistence.
An async analysis pipeline may submit a candidate, but it cannot approve or
activate that candidate directly.

Exact retry reuses the original approval, expected baseline, activation ID,
state ID, generation, and derived strategy identity. A changed stable head is a
stale conflict, not permission to adopt a newer baseline silently.

## Decision constraints

A definition declares deterministic decision constraints such as output type,
range, allowed values, step, fixed-baseline delta, target eligibility, required
inputs, and fallback.

Contract Service validates complete definition-owned constraints and candidate
compatibility before approval and activation. The target Phase 3 contract has no separately
resolved environment/operator constraint layer and no standalone Policy
service.

## Implemented manifest and approval contract

The sole authored input is
[`flaggo.decision-definition-bundle/v2`](../../../contracts/schemas/decision-definition-bundle-v2.schema.json).
Definitions own one result/default, context/targeting, required request/evidence
inputs, native OTel bindings, intent and current `policy` constraint data.
No producers, AST extraction, implicit hierarchy or rate/query DSL is present.

Read-only validation and apply enforce strict JSON/schema plus semantic checks:
duplicate/unknown fields, invalid defaults/steps, target conflicts, unknown
bindings, unsupported projections, source/type/unit/range/attribution errors,
freshness overflow and nonnumeric objectives are rejected. Required inputs
cannot use confirmed-exposure bindings, which would prevent the first decision
from creating its prerequisite exposure. Outcomes/objectives can still use
those bindings. The current reference-policy shape is rejected, not resolved.

Digests cover normalized `{ key, contract }`; owner/build/source metadata is
publication provenance. Opaque lineage/revision are server-issued.
New/changed semantics become immutable pending approval; identical or
metadata-only apply retains semantic identity. Exact approved apply replay
returns the stored receipt, including after restart. Reusing a key for another
canonical bundle conflicts.

Approval checks the exact digest and captured active-definition baseline.
Opposite terminal actions, expiry and stale baselines fail explicitly.
Expired resubmission is revalidated into one linked replacement request.
Current publication yields `status: "approved"` and exact accepted identities,
not future initial-authority activation facts.

Runtime, analysis and ingestion consume separate registry-owned typed
projections (`IRuntimeDefinitionReader`, `IIntelligenceDefinitionReader`,
`IEvidenceBindingReader`), never approval/persistence JSON. Both runtime and
analysis projections retain the same identity. Registry lookup remains
application/environment scoped; ingestion carries verified tenant scope into
materialization and never authorizes resource-attribute claims.

`LocalFileDefinitionRegistry` uses an exclusive cross-process lease, reloads
the complete state per operation and publishes atomically. Persistence version
2 retains both projections, apply outcomes, pending snapshots, baselines and
terminal decisions. Corrupt/old/unsupported/inaccessible storage fails closed,
without migration, seeded-field repair or fallback to a seed.
The [API baseline](../API_CONTRACT_PROPOSAL.md#management-api) lists exact
operations and snapshot integrity headers.

## Runtime projection

Decision Service receives a small immutable projection containing:

- complete runtime identity;
- target hierarchy and explicit fallback order;
- required runtime inputs;
- output contract and decision constraints;
- fallback value and behavior; and
- registration readiness.

The projection excludes bundle-management and approval internals.

## Failure behavior

Malformed definitions, conflicting lineages, unauthorized scope, invalid
constraints, failed approval, stale activation, and partial activation are
explicit non-ready outcomes. Contract Service never publishes a success-shaped
binding when required authority is absent.

## Current implementation mapping

The current `Flaggo.Registry` and control-plane host implement publication and
approval. #49 owns executable service/store/terminology alignment; #40 consumes
that baseline for initial-authority activation. Logical ownership does not
require one assembly or deployment per boundary.

## Related documents

- [Architecture overview](../../architecture/OVERVIEW.md)
- [Decision definition](../../architecture/DECISION_DEFINITION.md)
- [Decision authority](../../architecture/AUTHORITY.md)
- [State Store](../state-store/README.md)
- [Shared contracts](../shared-contracts/README.md)
