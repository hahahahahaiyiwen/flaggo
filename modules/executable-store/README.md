# Executable persistence and lifecycle

`flaggo-executable-store` implements the SQLite executable-store API.
With default features disabled it exposes portable lifecycle DTOs/errors,
the `ContractAuthority` semantic port, and shared `policy`/`artifacts` modules
without SQLx or SQLite. The default
`sqlite` feature adds `SqliteExecutableStore` and `SqliteContractAuthority`.
The latter constructs both logical stores over one pool and commits
accepted-ready publication and current/ownership-checked admission atomically. It is the
local management app's adapter; raw store writes remain lower-level test and
migration facilities, not a substitute for application admission.
Explicit initialization owns component schema **v6**. Existing executable-v4/v5
and contract-v3 state requires the explicit Contract Service scoped migration;
both logical stores migrate in one transaction. Failure rolls back both stores,
including copied admissions and schema metadata. Initialization never resets
executable state.

Local admission requires an injected `AdmissionFence`; without one it fails
closed. The workspace-backed guard excludes ownership/cycle transitions through
commit, including caller cancellation. Current submitter ownership is separate
from the original sealed Candidate provenance.

## Shared policy and adapter responsibilities

`policy::AUTHORITY_POLICY_CAPABILITY` is `flaggo.authority.policy/v1`.
Adapters must use these public functions rather than reimplementing product rules:

| API | Decision or validation |
| --- | --- |
| `artifacts::validate_new_executable` | Candidate/version-zero shape, checked-rule alignment, digest and canonical stored expressions |
| `artifacts::decode_persisted_artifacts` | Persisted contract binding, executable digest, checked DTO parsing/alignment and provenance decoding |
| `policy::validate_publication`, `plan_bootstrap` | Validate publication; default then authored bootstrap; preserve learned activation on redeploy |
| `policy::validate_admission`, `plan_admission`, `require_current` | Submission identity, immutable retry/conflict/lifecycle behavior and current-resource precondition |
| `policy::plan_activation` | Basic active-digest precondition and idempotent activation |
| `policy::validate_candidate_activation`, `plan_candidate_activation` | Manual candidate/current/active-token checks and next lifecycle token |
| `policy::validate_resolution`, `plan_resolution` | Automatic newest/current/ABA/rejection and already-resolved/yielded decisions |
| `policy::next_version`, `parse_state` | Lifecycle counter exhaustion and persisted state decoding |

Validation functions can run before opening a transaction. Plans consume state
read **inside the adapter's transaction/lock boundary**, not an earlier cached
snapshot. `LifecycleSnapshot` contains state/version; `ResolutionSnapshot`
also includes the admission timestamp, current contract, newest pending candidate
and active token. Newest ordering is documented on the snapshot and matches
the discovery ordering below. `ResolutionPlan` specifies no-op, stale-resource
supersession, rejection or activation; its `result` uses the actual committed
superseded count. An unsuccessful activation/rejection CAS must roll back
**all** earlier effects and return the shared yielded/conflict outcome.

Adapters own transactions, fencing, CAS, uniqueness, persistence encodings,
query execution/paging and cancellation/commit reconciliation. Supersession
effects target admitted candidates of the full resource, use checked version
increments and commit with activation/rejection. Shared helpers do not open a
transaction or certify a caller's snapshot. SQLite calls them inside its existing
immediate transactions; no remote transaction emulation is provided.

The artifact decoder accepts raw payload bytes and returns typed artifacts;
it has no row, connection, filesystem or timestamp-storage dependency. Preserve
checked-DTO duplicate-member validation by passing the bytes, not an already
collapsed JSON object. Native JSON faults remain distinct from integrity faults.
Checked-program materialization/AST validation still belongs to the existing
expression compiler/runtime; these helpers do not replace it.

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
- Automatic discovery chooses one candidate per full contract resource, preferring the
  current contract version, then newest admission and reference tie-breaks.
  Resolution preserves `Activated`, `Rejected`, `Superseded`, `Yielded` and
  `AlreadyResolved` outcomes. Active lifecycle versions detect ABA changes.
  Rejection does not require the active token to remain unchanged.

Superseding is scoped to admitted candidates for that full resource, not every
unadmitted candidate. Manual activation uses the maximum active/candidate version
for its next token; automatic resolution preserves the reference's separate
version rule. Failed transitions roll back deactivation and leave work pending.
Cancellation is explicit; transactional writes check it before commit.
Lifecycle tokens fail at integer exhaustion instead of wrapping or being reused.

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
cargo test --locked -p flaggo-executable-store --no-default-features --lib
cargo clippy --locked -p flaggo-executable-store --all-targets -- -D warnings
```

Tests cover concurrent admissions, basic/manual/automatic activation, stale
tokens/ABA, rollback, cancellation, pagination/lookahead, concurrent migration
and real SQLite-backed runtime evaluation. The differential gate supplies both
.NET- and Rust-produced checked artifacts as identical inputs to both stores;
valid standard protobuf encodings need not have identical compiler-produced
bytes. Read outputs are not normalized to hide those fields. Both producer
databases are read, reinitialized, transitioned and migrated by both runtimes,
with exact persisted-state snapshots and failed-migration rollback checks.
`flaggo-core-conformance::policy` exports a database-free policy/integrity corpus
and `c05_adapter_policy_parity` for comparing plans with committed adapter results.
The separate pinned consumer runs the corpus with no SQLx or SQLite dependency.

The differential gate uses an [explicit historical oracle](../../tools/core-parity/README.md);
it is retained historical evidence, not scoped v4 qualification.
Rust HTTP, emitted OTel and unchanged-client full-loop tests verify service
composition separately. Existing persisted state is retained after cutover.
