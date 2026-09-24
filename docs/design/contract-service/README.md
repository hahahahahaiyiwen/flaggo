# Contract Service design

## Purpose

Contract Service is the Flaggo control-plane service. It turns submitted
decision definitions into immutable registered contracts and turns approved
authority candidates into active decision state.

It owns the management API and the Contract Store boundary. It is the only
server service allowed to orchestrate authority activation.

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
compatibility before approval and activation. Phase 3 has no separately
resolved environment/operator constraint layer and no standalone Policy
service.

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

The current `Flaggo.Registry` and control-plane host are implementation modules
that provide parts of this boundary. Issue #40 owns executable consolidation
and contract migration. This document defines logical ownership, not a required
deployment split or assembly name.

## Related documents

- [Architecture overview](../../architecture/OVERVIEW.md)
- [Decision definition](../../architecture/DECISION_DEFINITION.md)
- [Decision authority](../../architecture/AUTHORITY.md)
- [State Store](../state-store/README.md)
- [Shared contracts](../shared-contracts/README.md)
