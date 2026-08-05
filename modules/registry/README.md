# Registry Module

Owns decision definitions, signal schemas, semantic identity, compatibility,
registration receipts, and approval snapshots.

The module exposes async ports for validation and immutable lookup. It must
recompute canonical digests, reject conflicting lineage or signal identity, and
apply bundles atomically. Runtime callers receive exact accepted identities;
they never select an older revision implicitly.

Update this document whenever normalization, lineage, compatibility, or storage
invariants change.

## Current implementation

`src/Flaggo.Registry` defines separate async runtime lookup, bundle-management,
and approval-management ports over one atomic in-memory adapter. Validation
recomputes RFC 8785 bundle and semantic contract digests, checks lineage,
signals, objectives, strategies, and policy structure, and performs no
mutation. Apply retains idempotent outcomes, immediately commits compatible
updates, and stores semantic changes as immutable pending snapshots.

Approvals compare the expected bundle digest before atomically committing a
new runtime revision. Each approval captures its active-definition baseline;
approval fails with a conflict if another approval changes that baseline first.
Approval and rejection are replay-safe terminal transitions; expiry and
opposite-terminal operations fail explicitly. Reference policies are rejected
until a policy-resolution adapter is available rather than being activated
without enforcement.
Runtime lookup continues to require the complete application, environment,
decision key, definition ID, and revision tuple.
