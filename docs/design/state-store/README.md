# State Store design

## Purpose

State Store is the durable authority boundary for one stable decision and
control-target address. It stores immutable activated behavior and exposes a
read-only runtime projection.

It is a store boundary, not a standalone product service.

## Persisted authority

Current state records contain:

- state identity and monotonic generation;
- application, environment, decision key, and control target;
- exact definition identity and contract digest;
- one `active-value` or `numeric-rule` authority payload;
- predecessor, proposal, activation, and approval references;
- activation timestamp and `active | superseded` status; and
- replay records that bind activation and proposal identities to state.

The stable address excludes definition revision so every replacement compares
and swaps one ordered authority head.

## Ports

Decision Service receives a read-only lookup port:

```text
exact definition identity + ordered resolution targets
  -> first compatible active state or no state
```

Contract Service receives activation ports:

```text
get captured baseline
activate approved candidate with expected-baseline CAS
```

No online caller or async analysis pipeline receives a direct state mutation
port.

## Invariants

- state and activation identities are nonempty, unique, and replay-bound;
- generations are contiguous within an authority address;
- the first generation has no predecessor and each replacement names the prior
  state;
- one authority address has one active head;
- exact replay returns the original state and derived strategy identity;
- stale activation cannot overwrite a newer head;
- publication exposes either the complete previous snapshot or complete
  replacement; and
- corrupt or incoherent state fails readiness instead of becoming fallback.

## Current implementation mapping

`Flaggo.State` owns the current in-memory and local-file ports and adapters.
Those libraries remain valid implementation details while #40 composes them
under Contract Service and Decision Service ownership.

## Related documents

- [Decision authority](../../architecture/AUTHORITY.md)
- [Contract Service](../contract-service/README.md)
- [Decision Service](../decision-service/README.md)
