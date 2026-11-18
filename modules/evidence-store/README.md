# Evidence contracts and local SQLite adapters

`EvidenceStore` owns semantic projection commits, deduplication/conflict effects,
diagnostics and the forward checkpoint. `EvidenceAnalysisStore` owns authorized
live scoped reads. Their public DTOs and errors can be consumed with
`default-features = false`, without SQLx, the SQLite query parser or local
execution dependencies. The default `sqlite` feature keeps both local adapters.

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
for implemented boundaries versus outstanding worker-fencing protocols.
