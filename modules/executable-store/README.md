# Executable persistence and lifecycle

`flaggo-executable-store` ports the SQLite executable-store API for issue #82
alongside unchanged .NET. Explicit initialization owns component schema **v5**,
transactionally migrates **v4** admissions, and preserves contract-store v3 and
other components. Failed migration rolls back its DDL, copied admissions and
component version. Initialization never resets executable state.

The store implements immutable candidate writes, per-workspace/cycle admission
idempotency, lifecycle reads, candidate/all-state keyset history, health and
`ActiveExecutableReader`. It validates checked-rule alignment and executable
digests; canonicalizer-backed writes additionally require already-canonical
stored expressions. Full contract/executable admission and checked-program
materialization remain separate compiler/runtime responsibilities.

Lifecycle writes use immediate transactions:

- Basic activation optionally fences the active digest; `activate_if_none`
  requires no active executable. Repeat activation is idempotent.
- Manual candidate activation fences candidate state/version, current contract
  and active digest/version, then activates and supersedes admitted backlog.
- Automatic discovery chooses one candidate per contract name, preferring the
  current contract version, then newest admission and reference tie-breaks.
  Resolution preserves `Activated`, `Rejected`, `Superseded`, `Yielded` and
  `AlreadyResolved` outcomes. Active lifecycle versions detect ABA changes.
  Rejection does not require the active token to remain unchanged.

Superseding is scoped to admitted candidates for that contract name, not every
unadmitted candidate. Manual activation uses the maximum active/candidate version
for its next token; automatic resolution preserves the reference's separate
version rule. Failed transitions roll back deactivation and leave work pending.
Cancellation is explicit; transactional writes check it before commit.

Stored reads preserve integrity versus native JSON/timestamp/argument/database
faults. Executable and provenance JSON use strict token validation. Checked DTOs
follow the reference's direct serializer instead: repeated members are allowed,
but each occurrence is type-checked before the last valid value wins. `checkedAst`
is decoded as a base64 byte array at this boundary, preserving native JSON faults
and canonical byte-array serialization rather than deferring malformed data to
materialization. Storage
timestamps and cursors reuse the contract-store compatibility helpers.
Lifecycle/outcome enums serialize as the reference's numeric low-level DTOs;
HTTP management representations are a separate composition concern.

```powershell
cargo test --locked -p flaggo-executable-store
cargo clippy --locked -p flaggo-executable-store --all-targets -- -D warnings
python tools\core-parity\run.py --profile --report .flaggo\core-parity\cel-report.json
```

Tests cover concurrent admissions, basic/manual/automatic activation, stale
tokens/ABA, rollback, cancellation, pagination/lookahead, concurrent migration
and real SQLite-backed runtime evaluation. The differential gate supplies both
.NET- and Rust-produced checked artifacts as identical inputs to both stores;
valid standard protobuf encodings need not have identical compiler-produced
bytes. Read outputs are not normalized to hide those fields. Both producer
databases are read, reinitialized, transitioned and migrated by both runtimes,
with exact persisted-state snapshots and failed-migration rollback checks.

This completes a library/store increment, not a service cutover. Rust HTTP,
emitted OTel, unchanged-client full-loop and removal gates remain separate.
