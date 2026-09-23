# Registry Module

This current internal library implements Contract Service and Contract Store
capabilities. It is not a separate first-class server component.

It owns decision definitions, signal schemas, semantic identity, compatibility,
registration receipts, and approval snapshots behind service-owned ports.

The module exposes async ports for validation and immutable lookup. It must
recompute canonical digests, reject conflicting lineage or signal identity, and
apply bundles atomically. Runtime callers receive exact accepted identities;
they never select an older revision implicitly.

Update this document whenever normalization, lineage, compatibility, or storage
invariants change.

## Current implementation

`src/Flaggo.Registry` defines separate async runtime lookup, bundle-management,
and approval-management ports over a shared lifecycle implementation. Validation
recomputes RFC 8785 bundle and semantic contract digests, checks lineage,
signals, objectives, strategies, and policy structure, and performs no
mutation. Apply retains idempotent outcomes, immediately commits compatible
updates, and stores semantic changes as immutable pending snapshots.

Accepted definitions are retained as two registry-owned typed projections with
the same canonical identity. `IRuntimeDefinitionReader` exposes only executable
runtime semantics: target hierarchy, inference target, explicit fallback
order, runtime-context requirements and target bindings, inputs, fallback,
action bounds, and runtime policy. `IIntelligenceDefinitionReader` exposes
objectives, signal roles, workflow permissions, action space, and safety
envelope for async intelligence and lifecycle consumers. Both projections are
persisted and reloaded together; consumers never parse registry JSON or
approval snapshots.

Legacy runtime-only v1 files migrate conventional target bindings only when
the persisted definition predates explicit targeting metadata, including the
legacy full fallback chain. New definitions that omit `fallbackOrder` authorize
no broader targets. Exact-identity seeded runtime safety fields may repair old
seeded records that predate those fields. Intelligence is restored only when
scope, complete canonical identity, and lifecycle status match a seeded
projection; otherwise intelligence lookup returns no definition rather than
fabricating objectives, permissions, or safety semantics.
When hierarchy and inference are both omitted, derived target precedence is
stable rather than JSON-property-order dependent: legacy target kinds use
`session`, `user`, `cohort`, `global` precedence and custom kinds sort
ordinally afterward.

Projection validation covers every typed field consumed by the builders.
Defaults and fallbacks must belong to their action space, including numeric
bounds and step alignment or string allowed-values membership. Metric
objectives must reference numeric signals, signal roles must reference declared
signals, and workflow modes must use the supported discriminants. The
intelligence `Allowed` set is derived from the same canonical evidence,
guardrail, inference-input, and objective references used by contract identity;
stale raw `signals.allowed` entries cannot change permissions independently.

Approvals compare the expected bundle digest before atomically committing a
new runtime revision and replacing the original apply idempotency outcome with
the approved registration receipt. Replaying that apply key and canonical
bundle therefore returns `200` after approval, including after adapter restart;
the same key with a different bundle remains a conflict. Each approval captures
its active-definition baseline;
approval fails with a conflict if another approval changes that baseline first.
Approval and rejection are replay-safe terminal transitions; expiry and
opposite-terminal operations fail explicitly. Reference policies are rejected
until a policy-resolution adapter is available rather than being activated
without enforcement.
Runtime lookup continues to require the complete application, environment,
decision key, definition ID, and revision tuple.
Inline cooldown constraints accept every finite nonnegative value preserved by
the frozen v1 schema. Runtime evaluation remains overflow-safe without adding
a compatibility-breaking maximum.

`InMemoryDefinitionRegistry` remains the deterministic unit-test adapter.
`LocalFileDefinitionRegistry` is the portable local shared adapter used by both
executable hosts. Every operation takes an exclusive cross-process lease,
reloads the complete registry state, and publishes mutations with a same-volume
temporary file plus atomic replace. The persisted state includes definitions,
apply idempotency outcomes, immutable approval snapshots, captured baselines,
expiry, and terminal decisions, so restarting either host does not weaken
lifecycle semantics. Reads reload under the same lease, making an approved
revision visible to already-running data-plane processes without polling or
domain-layer file access.

The persistence document is versioned and parsed strictly. Corrupt,
unsupported, inaccessible, or lock-starved storage fails explicitly; readiness
reports the registry unavailable rather than falling back to seeded state.
