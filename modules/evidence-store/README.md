# Evidence contracts and local SQLite adapters

`EvidenceStore` owns semantic projection commits, deduplication/conflict effects,
diagnostics and the forward checkpoint. `EvidenceAnalysisStore` owns authorized
live scoped reads. Their public DTOs and errors can be consumed with
`default-features = false`, without SQLx, the SQLite query parser or local
execution dependencies. The default `sqlite` feature keeps both local adapters.

`flaggo.evidence.projection/v2` requires a durable ownership epoch and expected
previous checkpoint. The SQLite adapter checks them inside the projection
transaction; catalog updates use the same epoch. Schema 5 adds ownership
metadata through the explicit materializer `--migrate-v4-to-v5` command, without
rewriting or replaying existing evidence. Ordered claim replacement is not a
time-expiring distributed lease.

## Shared projection policy

The following root exports are available with `default-features = false`;
SQLite reuses the same rules rather than maintaining adapter-specific policy:

```rust
validate_catalog(&StoredContractCatalog) -> Result<(), EvidenceStoreError>
validate_commit(&EvidenceMaterializationCommit) -> Result<(), EvidenceStoreError>
validate_observation(&EvidenceObservation) -> Result<(), EvidenceStoreError>
verify_observation_identity(&EvidenceObservation, &EvidenceObservation)
    -> Result<(), EvidenceStoreError>
plan_observation(
    &EvidenceObservationWrite,
    u64, // inbox batch ID
    Option<ExistingLogicalObservation<'_>>,
    Option<&EvidenceObservation>,
) -> Result<ObservationDisposition, EvidenceStoreError>
```

`ExistingLogicalObservation { observation_id: &str, content_digest: &str }`
represents the **first accepted row by logical_source_id alone**, without a
decoder/identity/projection version filter. This preserves current behavior:
same logical source/content is a duplicate across producer-version changes;
changed content conflicts rather than starting a new versioned logical key.
Logical lookup takes precedence over an observation-ID collision.

`ObservationDisposition` is `Insert`, `Duplicate`, or
`Conflict { conflict_id: String, diagnostic: EvidenceDiagnostic }`.
Insert records the observation and first provenance. Duplicate keeps both
unchanged. Conflict inserts once by conflict ID; insert its diagnostic only
when the conflict row is first inserted, preserving first conflict provenance.
The unchanged conflict hash uses length-prefixed UTF-8 parts:
`evidence-conflict/v2`, logical source ID, accepted content digest, rejected
content digest, decoder version, identity version, projection version.
Routing/materializer versions, observation IDs and retry provenance are not
conflict-hash inputs. Lookup identity and conflict identity are different.

When no logical row exists, adapters can supply the observation-ID row to the
planner, or attempt a unique insert and use `verify_observation_identity` on a
collision, as SQLite does. That comparison ignores routing-version differences
and ignores receipt timestamps only when **both** time sources are
`inbox.received_at`; other differing fields fail as corrupt data.

Adapters retain transaction-consistent lookups (including earlier writes in
the same batch), SQL row decoding, uniqueness, timestamps, ownership fencing,
checkpoint CAS and atomic commit/rollback. The policy does not perform I/O or
certify a remote adapter. `flaggo_core_conformance::evidence_policy::c09_projection_policy`
provides the database-free `flaggo.evidence-policy-conformance/v1` corpus;
the existing C09 adapter scenarios and SQLite parity tests remain separate.

## Query profiles

Queries explicitly select `flaggo.evidence.sqlite/v1`. Results use
`EvidenceValue` [canonical cells and the versioned saved-result envelope](../../docs/design/contracts/EVIDENCE_VALUES.md).
Provider type strings remain diagnostic metadata, separate from canonical
column kinds. Unknown expression types remain `variant`, including empty
results. The public crate includes the shared Python decoder source used by
local execution and available to other host adapters.

This does **not** make arbitrary SQLite SQL portable. The checked-in SQLite
profile corpus fixes scope/time, ordered projection, grouping, JSON, CTE, empty
results, numeric edges, NULL, overflow, collation and alias behavior. Another
adapter supplies its own labelled profile corpus to shared conformance tests,
not a relabelled dialect. Deadline expiry and dropping a query interrupt local
backend work; pool reuse is checked separately.

```powershell
cargo check -p flaggo-evidence-store --no-default-features --locked
cargo test -p flaggo-evidence-store --locked
cargo test -p flaggo-core-conformance --test sqlite_evidence --locked
python -B -m unittest discover -s modules\evidence-store\python -p test_*.py
```

See the [shared-core record](../../docs/design/architecture/CORE_EXTRACTION.md)
for implemented boundaries versus later hosted qualification.
