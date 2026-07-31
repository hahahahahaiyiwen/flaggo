# State Module

Owns governed decision state, cooldowns, overrides, pause/resume state,
exposure state, and rollback transition metadata.

State is keyed by immutable decision identity and control target. Semantic
contracts do not share governed state by default. Storage is accessed through
async module-owned ports so in-memory and durable adapters remain replaceable.

Update this document when lifecycle, concurrency, or persistence invariants
change.
