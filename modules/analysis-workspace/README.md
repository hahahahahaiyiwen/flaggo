# Analysis workspace contracts and local adapter

The portable `WorkspaceProvider`, `WorkspaceLease` and virtual filesystem
contracts describe cycle preparation, handoffs, budgets, candidate sealing,
recovery and relative artifact access. They do not require a host directory.
Use `default-features = false` to consume these contracts without the local
filesystem implementation, temporary-file dependency or local UUID allocation.

## Portable policy and codecs

`flaggo_analysis_workspace::policy` is available without default features. Its
serde snapshots retain the v1 local representation; they are storage facts, not
a required journal layout or distributed protocol:

| Operation | Shared facts and decisions |
| --- | --- |
| Prepare / complete | `CurrentCycle`, `StoredCycle`, `ScheduleState`, `StoredOutcome`; `plan_current_cycle`, `plan_eligibility`, `plan_outcome`, terminal contract/Candidate validation, `StoredCycle::terminal`, `ScheduleState::completed` |
| Seal / propose / recover | `AnalysisManifest`, `ManifestFile`, manifest membership/codec, `plan_proposal`, proposal codecs and immutable rules/manifest/record checks |
| Candidate receipt | Binding validation plus `plan_candidate_record` (`Append` or `Unchanged`; conflicting receipts fail) |
| Handoff | `HandoffRecord`, `plan_handoff` (optional append and pointer advance), binding/artifact validation and `workflow_handoff` |
| Execution | Existing request/receipt types, reservation and completion planners and receipt identity validation |
| Attempts | `CycleBudget`, `AttemptReservation`, reservation/settlement planners and `AttemptFailure` |
| Ownership | `ClaimRecord`, `CycleAccess`, `AdmissionIdentity`, claim epoch and admission owner/cycle/proposal predicates; `StoredCycle::superseding` |

Adapters supply the allocated cycle ID and time. Reconcile an existing terminal
outcome before scheduling or resuming; an identical completion keeps the journal's
original timestamp. Same-digest active **or superseding** cycles resume their
stored profile, but a manifest or pending proposal makes recovery read-only.
Changed digests finish the old cycle before replacement eligibility is considered.
Eligibility always uses `AnalysisContract::eligible_after`, including calendar
intervals. Local completion retains its outcome -> cycle -> schedule -> pointer
write order; these helpers do not prescribe that journal for other adapters.

Manifest entries use cycle-relative logical `/` paths, never host paths.
Adapters enumerate unique entries, validate paths/symlinks and hash actual bytes.
`AnalysisManifest::new` excludes the exact legacy control paths and sorts ordinally
by path. Its compact encoder preserves field order, including profile order and
`path`, `size`, `digest` entry order. Its decoder retains legacy serde acceptance;
`verify` checks schema, workspace/cycle/profile and full membership against a
freshly built manifest. Hash the **stored bytes**, not a re-encoding, for an
existing seal. Proposal encoding remains pretty JSON; recovery requires the
rebuilt manifest digest to match and retains the original proposal attempt.
`pending_proposal` decoding alone is not a membership/integrity verification.

The adapter must verify live ownership, current cycle, writable/sealed state and
artifact scope/byte integrity as appropriate **before** applying plans. Protect
authoritative reads and persistence with its transaction/CAS and lifetime fence.
`CycleAccess` and admission predicates only examine supplied facts; they do not
grant ownership. Admission uses the live claim's attempt/epoch separately from
the sealed proposal's original attempt, requires the active current cycle and no
terminal marker, and rejects superseding cycles. Retain admission protection
through the consuming transaction.

Metadata-free handoffs never advance the pointer; retries check predecessor and
content and cannot roll back newer workflow state. An existing pending execution
is uncertain, **not** a new grant; completion still requires the original epoch,
cycle and source attempt. Reservation plans charge before work; a crashed
reservation remains charged, and settlement refunds only the unused portion of
the matching reservation. Failure recording adds no separate charge or refund.
Unlike execution IDs, budget reservations are not idempotency keys: accepting
another reservation charges another attempt, even with the same attempt ID.

The policies have no filesystem, SQLx, app, transaction, lease-duration, backoff,
pause/cancel, billing or tenant-quota dependency. Pure consumer tests pin legacy
manifest/proposal bytes and the manifest digest and exercise replay/conflict
edges. The existing `flaggo-core-conformance::workspace::c10_c12_fenced_publication`
remains the adapter-level reusable suite; passing pure tests alone does not
qualify a provider's concurrency or durability.

## Local adapter

The default `local` feature supplies `LocalWorkspaceProvider`. Local Python
composition receives that provider separately; only its
`analysis_directory(&lease, cycle_id)` convenience resolves a host path. It
requires the exact live claim allocation from that provider, an active cycle
and writable/unsealed state. An equal workspace name/ID from another provider
does not grant access. Weak claim references do not retain ownership after a
lease is dropped.

File locks, atomic local publication, handoff-pointer recovery, proposal
sealing and existing budget behavior are retained. The local process remains
trusted and unsandboxed: resolving a local path is not a remote execution
protocol or isolation mechanism. Portable `flaggo.workspace/v2` claims bind
full resource identity and monotonic epochs. Immutable artifact handles,
conditional handoff publication and `flaggo.execution/v1` reservations/receipts
support recovery without replaying uncertain side effects.

The local provider also implements `AdmissionFence`. Its OS mutation gate
coordinates claim/cycle transitions with the Contract Service SQLite admission
commit. Both apps must share `FLAGGO_ANALYSIS_WORKSPACE_ROOT`. This local proof
does not qualify distributed lease expiry, cross-host storage or hosted execution.

Successfully acquired local file locks use an explicitly unlocking RAII guard,
not close-only release: on Linux, a concurrent fork can retain the same locked
open-file description until exec or exit, even with close-on-exec enabled.
Authority guards remain held for their existing mutation scope; admission guards
remain held through the owning transaction, including caller cancellation.
Lifetime claims, legacy-binding locks and successful idle-claim probes use the
same guard. Contention remains nonblocking, with no sleeps or retries. Explicit
claim release propagates unlock errors after revoking local capabilities;
drop-time unlock failures are logged.

Regression tests retain the real Rust-locked descriptor in a synchronized
Rust test subprocess to deterministically extend that inheritance window. They
check close-only contention, guard release/unwinding, admission exclusion and
claim recovery while the child remains alive. The ignored subprocess fixture is
invoked by those tests, not skipped coverage. Windows execution is not Linux
qualification; neither identifies a particular CI process-spawn interleaving.

```powershell
cargo test -p flaggo-analysis-workspace --locked
cargo test -p flaggo-analysis-workspace --no-default-features --locked
cargo clippy -p flaggo-analysis-workspace --all-targets --locked -- -D warnings
cargo clippy -p flaggo-analysis-workspace --no-default-features --all-targets --locked -- -D warnings
cargo test -p flaggo-core-conformance workspace:: --locked
cargo test -p flaggo-async-analysis --locked
```

Analysis tests require the checkout-local Python environment documented in
[`examples/README.md`](../../examples/README.md#python-environment-for-live-analysis).
