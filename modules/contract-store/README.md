# Contract-version persistence

`flaggo-contract-store` implements the Rust SQLite contract-version store for
the core services. The contract component
uses schema **v4**; the separate executable component uses **v6**.
`open` creates a connection pool, not a schema. Explicit `initialize` enables WAL,
checks the component version and creates missing contract tables without
resetting existing data or other components.

The default `sqlite` feature enables the local adapter. Disable default features
to consume accepted-version DTOs, errors and timestamp/cursor contracts without
SQLx. `ReadinessProbe` and runtime readers are implemented by the SQLite adapter.
Adapter-only `from_pool`, `put_in`, `get_in` and `set_current_in` let the semantic
authority adapter reuse existing persistence/integrity logic in one transaction;
applications must use `ContractAuthority::publish_ready`, not compose these
low-level writes themselves.

Writes claim full resource ownership atomically and insert immutable `(resource, digest)`
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
```

Focused tests cover immutability, ownership/current concurrency, pagination,
cancellation/rollback, timestamp boundaries, schema preservation and runtime
fault routing. The differential harness compares real SQLite operations,
reads both runtimes' databases with both readers, and checks that reinitialization
and restart preserve exact stored bytes, timestamps, resources and current
pointers. Historical .NET differential tests use an
[explicit historical oracle](../../tools/core-parity/README.md), not the scoped
v4 wire/schema qualification. Existing contract-v3/executable-v4/v5 databases
require the explicit, atomic Contract Service `--migrate-v3-to-v4` operation.
It preserves immutable bytes, digests, timestamps and activation across both
stores. Unsupported schemas fail startup; no reset or automatic reinterpretation
occurs. HTTP/OTel and full-loop compatibility have separate service-level tests.
