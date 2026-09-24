# Async analysis pipeline design

## Purpose

Async Analysis Pipeline is the worker-family boundary for analysis that may
produce future bounded authority candidates outside the online request path.

One deployment may host several pipelines with different schedules or
algorithms. They share the same authority submission boundary.

## Inputs and outputs

An analysis run may read:

- immutable definition and objective snapshots from Contract Store;
- observations, decisions, exposures, outcomes, and derived views from Evidence
  Store; and
- current authority state from State Store.

It may produce a bounded authority candidate plus rationale and evidence
references. It cannot approve the candidate, mint trusted lifecycle identities,
or write the active State Store head.

```text
contracts + evidence + current state
  -> bounded analysis
  -> authority candidate
  -> Contract Service approval and activation
  -> State Store
```

## Separation from runtime

Decision Service contains bounded deterministic execution for the current
request. Async Analysis Pipeline may perform longer-running or agentic work, but
its output remains a proposal and never enters the request path directly.

## Current scope

Phase 3 does not require an analysis implementation. Phase 4 issue #25 owns the
first proposal-managed lifecycle contract. This boundary exists now to keep
future analysis from being confused with runtime execution.

## Related documents

- [Decision authority](../../architecture/AUTHORITY.md)
- [Contract Service](../contract-service/README.md)
- [Evidence Store](../evidence-store/README.md)
- [State Store](../state-store/README.md)
