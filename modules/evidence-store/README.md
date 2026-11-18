# Evidence contracts and local SQLite adapters

`EvidenceStore` owns semantic projection commits, deduplication/conflict effects,
diagnostics and the forward checkpoint. `EvidenceAnalysisStore` owns authorized
live scoped reads. Their public DTOs and errors can be consumed with
`default-features = false`, without SQLx, the SQLite query parser or local
execution dependencies. The default `sqlite` feature keeps both local adapters.

This dependency separation does **not** make arbitrary SQLite SQL portable.
The existing query capability/result envelope still advertises SQLite syntax,
declared types and its established result behavior. The versioned negotiated
profile, lossless canonical value envelope and complete C11 corpus remain #94
work; a second provider must not claim compatibility by relabelling its dialect.

```powershell
cargo check -p flaggo-evidence-store --no-default-features --locked
cargo test -p flaggo-evidence-store --locked
```

See the [shared-core record](../../docs/design/architecture/CORE_EXTRACTION.md)
for implemented boundaries versus outstanding fencing and query protocols.
