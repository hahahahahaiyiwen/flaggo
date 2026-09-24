# Registry Module

Owns manifest definitions, evidence bindings, semantic identity, compatibility,
registration receipts, and approval snapshots.

The module exposes async ports for validation and immutable lookup. It must
recompute canonical digests, reject invalid or conflicting semantic identity, and
apply bundles atomically. Runtime callers receive exact accepted identities;
they never select an older revision implicitly.

Update this document whenever normalization, lineage, compatibility, or storage
invariants change.

## Current implementation

`src/Flaggo.Registry` defines separate async runtime lookup, bundle-management,
and approval-management ports over a shared lifecycle implementation. Validation
recomputes RFC 8785 bundle and semantic contract digests, checks lineage,
typed input ownership, native OTel bindings, objectives, and policy structure, and performs no
mutation. Apply retains idempotent outcomes, immediately commits compatible
updates, and stores semantic changes as immutable pending snapshots.

Accepted definitions are retained as two registry-owned typed projections with
the same canonical identity. `IRuntimeDefinitionReader` exposes only executable
runtime semantics: target hierarchy, inference target, explicit fallback
order, runtime-context requirements and target bindings, inputs, evidence, fallback,
action bounds, and runtime policy. `IIntelligenceDefinitionReader` exposes
objectives, typed evidence bindings, action space, and safety
envelope for async intelligence and lifecycle consumers. Both projections are
persisted and reloaded together; consumers never parse registry JSON or
approval snapshots.

`IEvidenceBindingReader` projects approved, non-retired bindings for the
authorized application/environment and carries the verified tenant scope
through to materialization. There are no producer declarations or inferred
target bindings. Manifest v2 and persistence version 2 replace older shapes;
unsupported data is rejected without migration, seeded-field repair, or
implicit hierarchy/fallback derivation.

Projection validation covers every typed field consumed by the builders.
Defaults and fallbacks must belong to their action space, including numeric
bounds and step alignment or string allowed-values membership. Numeric
objectives reference numeric evidence bindings. Evidence-owned operands inherit
the binding type/meaning/range; missing bindings, unsupported projections,
invalid attribution, and freshness overflow fail explicitly.

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
the manifest schema. Runtime evaluation remains overflow-safe.

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
