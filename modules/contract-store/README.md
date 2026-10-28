# Contract-version persistence

`flaggo-contract-store` implements the Rust SQLite contract-version store for
issue #82 alongside the unchanged .NET implementation. The contract component
uses schema **v3**; the separate executable component uses **v5**.
`open` creates a connection pool, not a schema. Explicit `initialize` enables WAL,
checks the component version and creates missing contract tables without
resetting existing data or other components.

Writes claim authority ownership atomically and insert immutable `(name, digest)`
versions. Repeated writes preserve the original payload and acceptance time.
An optional expression canonicalizer verifies and persists normalized expressions.
Like the reference low-level store, this API does not perform full admission
validation. Current selection is transactional; selecting a nonexistent version
leaves the current pointer unchanged. Ownership and current-selection writes
use immediate transactions to serialize competing writers.

Reads verify stored names, authority and content digests. Integrity errors remain
distinct from native JSON, argument, timestamp and SQLite failures. The
`ContractVersionReader` adapter preserves these distinctions for decision-runtime
fault classification, including native argument faults from non-finite typed
schema serialization. Cancellation errors are explicit and cancelled writes
roll back ownership and version changes.

History uses descending acceptance-time/digest ordering, page sizes 1 through
200 and reference-compatible base64 cursors. Storage/cursors use UTC timestamps
with seven fractional digits and `+00:00`; exact parsing also accepts the
reference's seven-digit `Z` form. Catalog ordering is SQLite ordering.

```powershell
cargo test --locked -p flaggo-contract-store
cargo clippy --locked -p flaggo-contract-store --all-targets -- -D warnings
python tools\core-parity\run.py --profile --report .flaggo\core-parity\cel-report.json
```

Focused tests cover immutability, ownership/current concurrency, pagination,
cancellation/rollback, timestamp boundaries, schema preservation and runtime
fault routing. The differential harness compares real SQLite operations,
reads both runtimes' databases with both readers, and checks that reinitialization
and restart preserve exact stored bytes, timestamps, resources and current
pointers. It also verifies coexistence with the actual .NET executable-v5 schema.
Executable-v5 persistence, v4 migration and activation are verified separately
by `flaggo-executable-store`. HTTP/OTel and full-loop compatibility remain
service-level gates. No service cutover or state reset occurs.
