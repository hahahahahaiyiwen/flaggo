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

`src/Flaggo.Registry` defines the async `IDefinitionRegistry` runtime lookup
port and an exact-key in-memory adapter. Runtime lookup resolves the complete
application, environment, decision key, definition ID, and revision tuple.
Registered revisions retain lifecycle status and inference input contracts so
retired identities and invalid signal values are rejected explicitly.
