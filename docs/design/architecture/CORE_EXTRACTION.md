# Shared-core extraction: authority, HTTP and workspace boundaries

Status: initial #94 extraction merged through #95 at
`db44dc9ea35c3c50109e2e65ba2a536eab753971`. Downstream adapter integration
identified a missed public-policy boundary; the acceptance correction below
is published and independently remote-consumer qualified, but does not inherit
#95's approval and has not been merged.
This is not hosted qualification.
Based on the approved architecture revision
`83088e825a0380a2a629b3bdd37cf8ffaaf1ae80`. The accepted design models in
[PORTABLE_CORE](PORTABLE_CORE.md) are distinguished from the concrete implemented
revisions recorded below.

## Implemented ownership and adapter contract

| Component | Responsibility |
| --- | --- |
| `flaggo-contract-application` | Shared validation, compilation, management HTTP/projections, Candidate admission and activation worker |
| `flaggo-decision-http` | Shared runtime HTTP, telemetry and injected readiness; uses the existing `DecisionRuntime` |
| `flaggo-otlp-http` | Shared OTLP transport, independent authentication, whole-batch authorization and durable append; no SQLite dependency |
| `flaggo-contract-store`, without default features | Accepted-version types, persisted-contract integrity decoder, errors, timestamp/cursor contracts |
| `flaggo-executable-store`, without default features | Lifecycle types/errors, `ContractAuthority`, shared `policy` decisions and `artifacts` integrity validation |
| `SqliteContractAuthority` | Local atomic authority adapter over one shared SQLite pool |
| Existing runnable apps | Local configuration, adapter initialization, listener and shutdown composition |
| `flaggo-core-conformance` | Reusable authority outcome scenarios, independent of SQL |
| `flaggo-analysis-workspace`, without default features | Path-free workflow and virtual filesystem contracts; local adapter remains feature-gated |
| `flaggo-analysis-application` | Shared coordinator, scoped Contract Service client, evidence/tools and observability with injected execution; no app or SQLite dependency |
| `flaggo-analysis-execution`, without default features | Path-free execution/reconciliation capability; default local adapter owns Python processes and host paths |
| `flaggo-evidence-materialization` | Shared decode, routing, identity and forward projection over inbox/store ports; no SQLite dependency |
| `flaggo-evidence-store` and `flaggo-raw-otlp-inbox`, without default features | Evidence/query and durable inbox contracts without SQLx or SQLite query-parser dependencies; existing local adapters remain defaults |

`ContractAuthority` v3 uses an explicit validated
tenant/application/environment/name resource identity. Reads and mutations
must share one authority domain. `publish_ready` commits resource ownership,
immutable contract and bootstrap artifacts, activation and management current
together. `admit_current` checks currentness and retries inside the same
transaction as artifact/provenance insertion. Candidate/active state-token
activation and automatic resolution retain their existing semantic outcomes.
Integer exhaustion fails rather than wrapping lifecycle tokens.

Candidate submissions carry `ownership: { attemptId, epoch }` separately from
sealed provenance. The epoch is canonical decimal text, not a JSON number.
Recovery uses the new owner's claim while retaining the original proposal's
attempt and manifest. Admission validates current ownership and the authorized
active cycle even for an already admitted candidate.

Local Contract Service injects the workspace provider as `AdmissionFence`.
An OS-backed mutation gate excludes claim replacement, acknowledged release
and cycle closure through the SQLite admission commit. The provider verifies
resource binding, live lifetime lock, epoch, current cycle, contract digest and
sealed proposal identity. Request cancellation does not drop the gate while
SQLx finishes a queued commit. A missing fence fails closed; standalone adapters
must supply equivalent coordination, not a preflight-only check. Both local
processes must use the same `FLAGGO_ANALYSIS_WORKSPACE_ROOT` (default
`flaggo-analysis-workspaces`). This is local coordination, not distributed
lease expiry or cross-host filesystem qualification.

SQLite shares the existing persistence and integrity routines inside those
transactions; the app no longer sequences separate writes or uses a
process-local current/admission mutex. A failed operation is not a claim that a
remote commit rolled back: future adapters must preserve explicit dependency
errors and reconciliation guarantees. No remote retry protocol is certified here.

### Acceptance correction: reusable product policy

At merged #95, `ContractAuthority` was portable but bootstrap/redeploy,
activation/resolution and persisted-executable validation still lived under the
SQLite feature. That was a missed #94 acceptance boundary: a second adapter would
otherwise copy product rules. The additive `flaggo.authority.policy/v1` capability
now exposes those decisions in `flaggo_executable_store::policy` and artifact
checks in `flaggo_executable_store::artifacts`, with SQLite calling the same code.

`plan_bootstrap` returns ordered default/authored steps without resetting learned
authority. `plan_admission`, `plan_activation`, `plan_candidate_activation` and
`plan_resolution` consume transactionally read state and return decisions rather
than performing I/O. Resolution retains all five outcomes, active-token ABA
checks, rejection independence and the existing distinct manual/automatic
version rules. Adapters keep atomicity, ownership fences, CAS and full rollback.
The byte-oriented artifact decoder preserves checked DTO parsing and error
classification without requiring SQLite rows or its timestamp representation.
See the [exported seams and adapter invariants](../../../modules/executable-store/README.md).

The same correction also exports
`flaggo_contract_store::artifacts::decode_persisted_contract(resource_key, digest, payload)`.
It verifies original accepted-contract bytes, full resource identity/scope and
digest without SQLx; SQLite reuses it and retains timestamp/row decoding.
Shared conformance and SQLite corruption/error-parity cases exercise the seam.
Existing public cursor codecs carry only timestamp/digest, not scope.
Authorization and scoped resource/exact-contract queries provide isolation;
neither token encoding nor cross-scope token acceptance changes here.
Capacity admission (429 plus Retry-After) remains a separate, pending shared
HTTP behavior change and is not included in this pure-policy correction.

Shared `flaggo.authority-policy-conformance/v1` scenarios exercise those decisions,
persisted-artifact integrity and policy-versus-committed-adapter parity. The
standalone consumer additionally executes the database-free corpus. Existing v4
wire formats, digests, schemas and `flaggo.authority/v3` are unchanged. Historical
remote consumer pins below do not qualify this correction; its exact local or
published revision and consumer evidence must be reported separately.

The executable-policy correction's earlier source revision is
`79974bb950f90b65dfae0b82b7a3b7b7cd2aa341`. Focused executable-store,
Contract Service, Decision Service and shared conformance tests pass (56 passed,
two historical fixtures ignored); no-default-feature checked-parser tests and
strict Clippy also pass. A fresh standalone **local Git** consumer executes the
shared policy/integrity corpus, canonical/evaluation checks and compositions:
all 20 product crates resolve to that exact revision, with no SQLx/SQLite or
runnable-app dependency. Its retained lock SHA-256 is
`4a1e3448f07d7bde85ca3fe7c4e04990697f56418752bc5bb14bd4cdba5d3bec`;
manifest, lock and result are in session `files/pinned-consumer-policy-79974bb`.
This earlier run is not remote publication, private-adapter integration or
distributed qualification. Its publication was pending at that milestone;
the main-based public qualification is recorded below.
This earlier pin does not qualify the subsequently added accepted-contract
decoder; its exact source and independent consumer must be recorded separately.

The cumulative correction including the accepted-contract decoder is locally
qualified at `5cf6aab8dab98e846753b3993cc98538d6aef359`: the five-package
contract-store/executable-store/Contract Service/Decision Service/conformance
suite passes **65 tests**, with **2 historical fixtures ignored**. Strict
all-target Clippy and no-default-feature store checks pass. A fresh standalone
local Git consumer executes both persisted-integrity corpora and pins all
**20** product crates to that revision without SQLx/SQLite or runnable apps.
Its lock SHA-256 is
`6b7d7bc794e39436d7451bad0ae0bd0f31bab56e4388b3c4b1bb27f39031ab63`;
the manifest, lock and result are retained in session
`files/pinned-consumer-contract-5cf6aab`. This remains historical local-only
qualification, separate from the main-based public remote consumer below.

### Main-based correction publication

The four focused correction commits were reapplied, in order, onto current
`origin/main` at `db44dc9ea35c3c50109e2e65ba2a536eab753971`, producing
`f873ad91aa97771845ac4974628385a3f86c44d3`. After explicit publication approval,
that revision was non-force pushed to the public repository on branch
`hahahahahaiyiwen-portable-policy-correction`; no merge was performed.

At that exact revision the five-package suite passes **65 tests**, with
**2 historical fixtures ignored**, and strict all-target Clippy, formatting,
no-default-feature store checks and all **4** portable checked-parser tests pass:

```powershell
cargo test --locked -p flaggo-contract-store -p flaggo-executable-store -p flaggo-contract-service -p flaggo-decision-service -p flaggo-core-conformance
cargo clippy --locked -p flaggo-contract-store -p flaggo-executable-store -p flaggo-contract-service -p flaggo-decision-service -p flaggo-core-conformance --all-targets -- -D warnings
cargo check --locked -p flaggo-contract-store -p flaggo-executable-store --no-default-features
cargo test --locked -p flaggo-executable-store --no-default-features --lib
cargo fmt --all --check
python tools\core-consumer\check.py --repository https://github.com/hahahahahaiyiwen/flaggo.git --revision f873ad91aa97771845ac4974628385a3f86c44d3 --output <new-external-directory>
```

The fresh **public remote Git** consumer passes both persisted-integrity corpora,
shared lifecycle policy, canonical/evaluation checks and HTTP compositions.
All **20** product packages resolve to that exact revision, with no SQLx,
SQLite or runnable-app dependency. Its retained lock SHA-256 is
`7a54ba73aa185f100acfda6ac615cddc786c15ecc0cdf5228e4f6663b517ddb2`.
The generated manifest, lock and result are retained in publication session
`b738c4a7-a548-401b-afc7-9875b7c2fada`, under
`files/remote-consumer-f873ad9`. This publication record is documentation-only;
subsequent exact-head consumer and CI results belong to the correction PR.
Public consumption does not establish private-adapter adoption, hosted
qualification or multi-host readiness. Capacity admission remains separate.

Core crates disable store default features. SQLx and SQLite are absent from
their normal dependency graph. Local store defaults still enable SQLite for
existing runnable apps. `ReadinessProbe` returns capability availability or
typed failure without scanning artifacts. Active selection remains authoritative
per request; compiled immutable programs alone are cached.

`WorkspaceLease` no longer requires `PathBuf` or a host-local execution
directory. The local capability factory receives the local provider separately;
its execution-path projection accepts only the exact live claim from that
provider and preserves active/writable/sealed-cycle checks. The provider keeps
weak references so projection cannot extend ownership. This separates the
local convenience without treating a host path as an execution contract.

`flaggo.workspace/v2` binds identity to the full resource, uses persisted
monotonic claim epochs, and revokes derived capabilities on explicit release or
drop. Released capabilities cannot read, write, settle budgets, publish handoffs
or finalize executions. Overflow fails closed. The local adapter uses OS lock
exclusion, not expiring multi-host leases.

Immutable artifact references bind resource, workspace, cycle, attempt, content
digest and byte length. Upload does not commit a workflow transition.
Publication compares the previous handoff identity under the live fence;
identical retries reconcile the record/pointer boundary, while stale revisions,
cross-scope references and changed bytes fail. Metadata-free checkpoints do not
advance the workflow.

`flaggo.execution/v1` reserves a scoped execution idempotency key before running.
Completed receipts reference immutable results; retries reconcile without
repeating side effects. Pending receipts represent a possibly executed request,
not permission to rerun. A replacement fence cannot finalize that old execution.
Copilot invocation identity supplies the stable retry key; direct tool calls
without a supplied key deliberately create a new execution. Local Python remains
trusted host execution, with bounded captured output and cancellation; it is
not the malicious-tenant sandbox required for hosted qualification.

The evidence and raw-inbox crates likewise separate their contracts from
optional local adapters; the OTLP codec no longer enables SQLite transitively.
Their local defaults remain unchanged. The subsequent query slice integrates
explicit versioned profile selection, canonical cells, saved scope/read/limit
metadata and Python/deterministic consumers. Ingestion now commits the unchanged
payload with `flaggo.otlp.authorization/v1` producer/scope metadata only after
every Resource has been authorized. Replay rechecks that envelope. Portable
projection fencing uses `flaggo.evidence.projection/v2`: durable monotonic claim
replacement and expected-checkpoint comparison are checked in the same
transaction as every projection. Catalog updates check the same claim.
Starting a local materializer explicitly replaces ownership; a stale worker
fails closed rather than reacquiring on each poll. This is ordered replacement,
not hosted time-expiring lease or failover qualification.

## Implemented breaking wire cutover

After reviewing this slice, the user selected
**"Use v4 for the full-scope API (Recommended)"** and
**"Replace v3 with an explicit breaking v4 cutover (Recommended)"**.
The implemented route is:

```text
/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}
```

Current runnable apps serve v4 only. Schemas, OpenAPI, generated SDK artifacts,
fixtures, runtime configuration v2 and local clients use the explicit scope.
Unchanged contract content retains its digest. HTTP composition requires an
independently injected `RequestSecurity`; the local binaries explicitly select
`TrustedLocal`. Missing/invalid identity returns 401, hidden scope 404, visible
forbidden operation 403, and unavailable authorization authority 503. Declared
scope is never a credential or an implicit alias selected from claims.

Stop local writers and back up the shared database before explicit migrations:

```powershell
$env:FLAGGO_CONTRACT_DATABASE_PATH = "<existing local database file>"
cargo run -p flaggo-contract-service -- --migrate-v3-to-v4
$env:FLAGGO_DATABASE_URL = "<SQLite URL for that same file>"
cargo run -p flaggo-otel-ingestion -- --migrate-v1-to-v2
cargo run -p flaggo-evidence-materializer -- --migrate-v4-to-v5
```

The evidence migration adds schema-5 ownership metadata while preserving exact
observations and existing checkpoints. It does not replay prior batches.
The first operation atomically migrates contract schema 3 and executable schema
4/5 to 4/6, deriving scope from existing accepted content. It preserves exact
artifact/provenance bytes, digests, timestamps and lifecycle state; any invalid
ownership or reference rolls back both stores. Neither migration starts HTTP.
Startup refuses unsupported schemas rather than guessing or silently resetting.
The inbox migration preserves legacy bytes and receipts with a **missing**
authorization envelope, never fabricated past authentication. Such historical
unverified batches produce an authorization diagnostic rather than new evidence;
authorized re-ingestion is required to project them. Materializer decoder
revision 5 does not claim that existing projection/checkpoint revisions were
automatically replayed. Keep sealed historical runs unchanged.

Before using a pre-scoped local workspace, stop its writer and explicitly bind
the historical name-only directory:

```powershell
$env:FLAGGO_ANALYSIS_WORKSPACE_ROOT = "<existing workspace root>"
$env:FLAGGO_ANALYSIS_TENANT = "<tenant>"
$env:FLAGGO_ANALYSIS_APPLICATION = "<application>"
$env:FLAGGO_ANALYSIS_ENVIRONMENT = "<environment>"
cargo run -p flaggo-async-analysis -- --bind-legacy-workspace <contractName>
```

This command runs before any agent/provider initialization. It checks every
stored cycle's scope, preserves the old directory/workspace ID and exact sealed
bytes, and records an explicit sidecar binding. It refuses incompatible scopes
or an existing separate scoped history. Unbound legacy history is never silently
abandoned. New workspaces use the `flaggo.workspace/v2` full-resource hash domain.

## Compatibility record

| Surface | This slice |
| --- | --- |
| Product revision | The Git commit containing this record; consumer runs additionally record their exact immutable source SHA in `result.json` |
| Public HTTP and SDK | Management/runtime **v4**, explicit full scope; deployment schema v3 retains manifest v2 and requires runtime configuration v2 |
| Expression/canonical identity | `flaggo.cel/v1`; unchanged contract/executable digest algorithms and fixtures |
| SQLite schemas | Contract **4**, executable **6**, evidence **5**, raw inbox **2**; explicit authority/inbox/evidence migrations, no reset |
| Authority capability | `flaggo.authority/v3`: full-scope atomic ready publication/fenced current admission, scoped catalog/pagination and activation/read contracts |
| Shared authority policy | `flaggo.authority.policy/v1`: SQLx-free transition plans and persisted-artifact validation; `flaggo.authority-policy-conformance/v1` corpus and adapter parity |
| Security capability | `flaggo.security.resource/v1`, independently injected policy; `flaggo.otlp.authorization/v1` retained producer/scope envelope |
| OTLP profile | `flaggo.otlp-http` **2.0.0**; standard OTLP payloads/endpoints, whole-batch authorization, explicitly local numeric defaults |
| Reusable fixture revision | `flaggo.authority-conformance/v3` (C07 namespace and C03-C05), `flaggo.admission-conformance/v1` (C04/C10 ownership/recovery), `flaggo.evidence-conformance/v3` (local C08/C09/C11 plus projection fencing outcomes) |
| Canonical/runtime corpus | `flaggo.canonical-conformance/v1`, `flaggo.runtime-conformance/v1`; shared canonical/evaluator cases and injectable exact-reader/cache/failure scenarios |
| Projection capability | `flaggo.evidence.projection/v2`, explicit ordered claim replacement, atomic epoch/checkpoint preconditions; materializer revision 3 |
| Evidence query | Explicit `flaggo.evidence.sqlite/v1` selection and `flaggo.evidence.query-result/v1` saved artifacts; provider SQL semantics are labelled, not arbitrary-SQL portable |
| Canonical evidence cells | `flaggo.evidence.value/v1` integrated into typed rows, Python/agent guidance and deterministic consumers |
| Workspace/execution | `flaggo.workspace/v2`, `flaggo.execution/v1`, `flaggo.workspace-conformance/v1`; local fenced publication, durable execution reconciliation and explicit trusted-local Python profile |
| First hosted slice | Not qualified: private identity/distributed adapters, isolation and actual multi-host scenarios remain required |

Concurrency strengthens persistence without changing PUT retry semantics:
identical redeployment preserves acceptance identity and learned activation;
successful publication selects current, including an old-version redeploy.
Admission still checks currentness **before** duplicate reconciliation, returning
the stale-contract response. Activation tokens and exact runtime semantics are
retained; resource-key pagination and catalog ETags are explicitly scope-bound.
Bootstrap success telemetry is emitted only after the combined commit succeeds.

## Adding an adapter

Implement `ContractAuthority` against semantic atomic operations, never an app
mutex or a sequence of independently committed CRUD calls. Supply authoritative
runtime readers and readiness probes. Use the shared application/HTTP crates
instead of importing app binaries or copying lifecycle/HTTP implementation.
Keep provider types and diagnostics in the adapter; preserve invalid input,
conflict, integrity, unavailable, timeout and cancellation distinctions.

Implement `AuthorityFixture` with clean storage, independently constructed
clients and publication-failure injection. Run every advertised scenario and
record unsupported capabilities separately. The SQLite fixture demonstrates
one implementation, not a requirement for physical tables or SQL.

Run `canonical::c01_canonical` in every independently pinned consumer. It covers
published digest identity, named-collection ordering, descriptions versus literal
content, rule ordering, scope changes, expression normalization, exact numeric
boundaries, malformed JSON and ordered-rule/default evaluation.
Implement `runtime::RuntimeFixture` to run `c02_exact_runtime` against real
readers: changing management current cannot select a different exact version,
scope mismatches fail, outages never use cached activation, and persisted
corruption fails on both cached and fresh runtimes.

Run `admission::c04_c10_fenced_admission` with independent authority clients
and a workflow provider coordinated with admission authority. It checks
released/replaced ownership, recovery without provenance relabelling, exact
retry reconciliation and closed-cycle rejection. The local adapter additionally
tests cancellation while SQLite is blocked: ownership release cannot succeed
until the in-flight admission finishes.

Implement `WorkspaceFixture` with independently constructed providers over
shared clean storage. Run `c10_c12_fenced_publication` for claim exclusion,
conditional publication, stale-capability rejection, completed-result recovery
and pending-execution non-replay. Execution adapters implement `ExecutionProvider`
and must honor durable reservation before starting work. Local conformance is
not evidence of multi-host expiry, scheduler isolation or hosted sandbox safety.

Focused checks:

```powershell
cargo test -p flaggo-contract-service -p flaggo-decision-service -p flaggo-contract-store -p flaggo-executable-store -p flaggo-decision-http --locked
cargo check -p flaggo-contract-application -p flaggo-decision-http --locked
cargo clippy -p flaggo-contract-application -p flaggo-decision-http -p flaggo-core-conformance --all-targets --locked -- -D warnings
cargo test -p flaggo-async-analysis packaged_skills_override --locked
cargo test -p flaggo-analysis-workspace -p flaggo-async-analysis -p flaggo-evidence-store -p flaggo-raw-otlp-inbox -p flaggo-otlp-codec --locked
cargo test -p flaggo-analysis-application -p flaggo-analysis-execution -p flaggo-core-conformance --locked
cargo check -p flaggo-analysis-agent -p flaggo-analysis-workspace -p flaggo-evidence-store -p flaggo-raw-otlp-inbox -p flaggo-otlp-codec --no-default-features --locked
npm run test:tetris-full-loop
```

The [separate consumer](../../../tools/core-consumer/README.md) compiles all three HTTP
compositions, runs a canonical fixture and evaluation against a Git revision,
retains a lockfile and rejects accidental SQLite dependencies. An explicit local
Git URL proves local immutable consumption only. It is not evidence that the
revision is publicly reachable. Public remote pinning must be verified after
authorized publication.

## Selective historical reconciliation

Reviewed changes against `c397329f608c04631d309f2cfff966a58d80fe2c` from
`hahahahahaiyiwen-flaggo-website`; no wholesale merge or historical relabelling.

| Historical change | Decision |
| --- | --- |
| Packaged analysis skill-root override, all-profile startup validation and focused test | Included: reusable consumers must not require the original build checkout's skill directory |
| Tetris active-cycle/provenance observer correlation and latest-failure display | Deferred: presentation change, not required for shared authority extraction |
| Browser fresh-human-session setup and workload-specific telemetry assertions | Deferred with their browser/rehearsal consumers; not silently substituted for this slice's deterministic local gate |
| Linux wheel lock/provenance and SDK runtime probe | Deferred: sealed historical preparation/runtime diagnostics, not shared product capability implementation |
| Azure workflow, Bicep, gateway, service units, sealed release/replay, diagnostics and recovery scripts | Excluded: deployment ownership and no Azure operations requested |
| Unrelated website work | Untouched |

Historical runtime source remains
`b109a917d48c0bd63ecbc4260a8dd957bfab3ffd`; historical driver remains
`1354e61bb0fa2698056ac9bcbf57f5a60f5df5d5`. New local builds are not those
artifacts and cannot inherit their diagnostics or deployment identities.

## Conformance coverage and remaining qualification

| Gate | Actual state |
| --- | --- |
| C01-C02 | Shared canonical/evaluator corpus and reusable exact-runtime fixture implemented; real SQLite current-pointer/cache/outage/corruption scenarios pass. Pinned consumer executes canonical/evaluation parity; other adapters must run the same corpus |
| C03-C05 | Reusable local publication/failure/admission/activation outcomes implemented; store ABA/rollback coverage retained; candidate ownership gate additionally checked while SQL is blocked/caller cancelled. Actual distributed barrier/failure qualification remains phase 3 |
| C06 | Local injected-reader behavior only; multi-host acknowledgement/outage qualification remains phase 3 |
| C07 | Scoped v4 keys/routes/SDK/artifacts, injected authentication and local migration implemented; namespace, security denial/no-write and migration preservation/rollback tests pass. Private identity adapter qualification remains later |
| C08-C09 | Reusable durable append, fenced atomic projection/checkpoint, retry/dedup/conflict outcomes and authorized whole-batch envelope implemented; replay rejects missing/mismatched authorization. Actual distributed qualification remains later |
| C10 | Local workspace/projection epochs and authority-verified admission implemented; reusable recovery and real local commit-exclusion/cancellation checks pass. Multi-host expiry/skew qualification remains later |
| C11 | Explicit result/profile selection, canonical cells, scope/read/limit/artifact metadata and Python/agent consumers integrated. Shared twelve-category SQLite profile corpus runs under two scopes; local deadline/cancellation and reuse checked. Actual second-provider qualification remains later |
| C12 | Path-free execution provider, immutable conditional publication, durable receipts and reusable recovery scenarios implemented; hosted isolation and cross-host execution qualification remain later |
| C13-C14 | Hosted isolation, operational envelope, migration/restore/data-lifecycle qualification remain later gates; no numeric policy inferred |
| Consumer/publication | Complete 20-crate immutable local and public remote consumers verified below; both retain distinct locks and exact source revisions |

## Issue #94 acceptance record

| Criterion | Evidence |
| --- | --- |
| Shared application/domain logic and explicit capabilities | Management/runtime/OTLP HTTP, analysis coordinator/tools and evidence materialization are public libraries; local apps compose SQLite, workspace and execution adapters |
| Local behavior and deliberate compatibility changes | Scoped v4/schema migrations are explicit; complete Rust, SDK/wire and deterministic real-host closed-loop gates pass |
| Reusable adapter conformance | Canonical/runtime, publication/admission/activation, scoped evidence/query and workflow/execution recovery scenarios exported by `flaggo-core-conformance`; provider fault controls stay in fixtures |
| Portable workspace/execution with retained local safety | Path-free handles, conditional artifacts, revocable epochs, cancellation-safe admission coordination and durable execution receipts; local trusted execution remains explicit |
| Separately pinned consumer | Local `caab95c7623fafbe8c5fcdfb25f2d324085ca80d` and public remote `3a49467692aa282769b83459fe5741a536eb6429` build independently with 20 uniformly pinned public crates and no SQLite/app dependency |
| Contributor guidance | This guide, `CONTRIBUTING.md`, module READMEs and `tools/core-consumer/README.md` document ownership, adapter invariants and focused checks |
| Revision/API/schema/capability record | Compatibility table above and consumer `result.json` bind the exact implemented revisions; hosted-only qualification is not implied |

## Verification record

The completed local core additionally passes the entire Rust workspace suite and
strict all-target Clippy, all 76 SDK tests, SDK/example type checks, generated
artifact checks, eight Python conformance tests and all 1,201 offline checks.
Ignored historical/live fixtures remain ignored, not reported as passes.
The final deterministic real-host Tetris rerun also passes at source
`caab95c7623fafbe8c5fcdfb25f2d324085ca80d`: 15 retained OTLP batches,
1,004 evidence observations/provenance rows, zero conflicts/diagnostics,
reviewed deterministic analysis, fenced admission, activation, SDK/REST parity
and restart persistence with activation metadata preserved. Its activated
candidate is
`sha256:285acca3c2da217ccac773bea23ca041b3a7b9ec6c9d430e5574fb1a57db5877`.
No model provider or cloud service was used.

The final separate local Git consumer pins the same source revision, with
**20** uniformly pinned product crates and no SQLx/SQLite or runnable-app
dependency. It executes the shared C01 corpus, C02 exact-evaluation parity and
canonical evidence-cell checks, and compiles the three HTTP compositions plus
analysis/execution/materialization contracts. Its retained `Cargo.lock` SHA-256
is `ba3c18fc1035ce4f5b01fb39c6cb3990a64decca954e17f760b16c071dcd5ea2`.
This is **local Git consumption**, not remote publication. The release-review
cleanup restores baseline line endings without changing logical source content.

After explicit publication approval, branch
`hahahahahaiyiwen-shared-core-extraction` was pushed to the public
`https://github.com/hahahahahaiyiwen/flaggo.git` repository. A fresh standalone
consumer then fetched and passed at remote revision
`3a49467692aa282769b83459fe5741a536eb6429`, independently of the earlier
file-URL source. All **20** product packages resolved to that same remote
revision; the graph contains neither SQLx/SQLite nor runnable apps.
Its retained lock SHA-256 is
`514dc77d4e8ffc2f85c9c0839f9dbef4d474c71a13a76b4e9c279402e7e13ef3`.
The generated manifest, lockfile and `result.json` are retained in the session's
`files/pinned-consumer-remote-3a49467` evidence directory. The subsequent
`ab3a1c19735cc5827e7ffe6129a3ec2dae014749` publication record is documentation-only.
At that publication milestone no PR, merge, issue closure or cloud deployment
had been performed.

### Pull request review

[PR #95](https://github.com/hahahahahaiyiwen/flaggo/pull/95) includes the approved
#93 architecture gate and this implementation. Review reproduced and corrected
a local admission race: marking a cycle as superseding must hold the same
cross-process authority gate as cycle closure and ownership release. The
blocked-SQL/caller-cancellation regression now proves supersession cannot
acknowledge before admission finishes, and that admission retries fail after
supersession acknowledges. The regression failed before the fix and passes
after it; local workspace tests and focused strict Clippy also pass.

Linux CI additionally exposed a nanosecond-clock admission response mismatch.
New admissions now return the same persisted 100-nanosecond timestamp as
idempotent retries, without changing the existing storage or wire format. A
fixed nanosecond input reproduces the failure on every host and verifies the
correction. Browser closed-loop assertions now verify the scoped execution
source artifact, its cycle's contract binding, digest and byte length instead
of the replaced host-local submission shape.

The earlier immutable consumer records above retain their exact historical
meaning. Follow-up remote consumer pins and Linux CI outcomes are recorded on
the pull request; they must not be inferred from the earlier qualification.

### Earlier slice checks

The scoped-v4/authorized-ingress slice passes the complete workspace suite
(330 tests, four explicitly ignored historical/live fixtures), workspace Clippy,
76 SDK tests, nine focused deployment/provider tests, and 1,201 offline
wire/profile/capability checks. The four published OTLP fixtures are also
dispatched through the actual shared receiver, including scoped binary/gzip
payloads. The deterministic real-host Tetris loop verified scoped deployment,
SDK/REST parity, authorized telemetry, proposal/review/admission, activation
and restart persistence. These are local results, not distributed qualification
or a new remote consumer pin. The subsequent workflow slice passes scoped
migration/preservation, release/reacquisition, conditional artifact publication,
receipt reconciliation, real-Python no-repeat execution, local workflow
regressions and strict all-target Clippy.

The source pinned by the successful standalone consumer is
`ef2541081231a2e12c738731a6bcf14bbf8d33c3` (authority/HTTP extraction plus its
consumer-fixture correction). The build used an explicit local Git URL, not a
remote branch or a path dependency. Its retained `Cargo.lock` SHA-256 is
`c3f39fd7fd2b2d255ecd1cc00e16cbc40fc6e4b890d93d22e378093600b069df`.
All ten transitive product crates resolved to that same Git revision; metadata
contained no SQLx/SQLite package. Both HTTP compositions compiled, the canonical
fixture digest matched, and exact-contract evaluation returned the expected
default. That earlier file-URL run did **not** test public remote reachability.

The expanded consumer subsequently passed at
`587985c2ec6e65be72c6bf32ec3658d84a0b0b77`, compiling path-free workspace,
evidence and raw-inbox ports as well. All **14** transitive product crates
resolved to that immutable local Git revision, with no SQLx/SQLite package.
Its retained lockfile SHA-256 is
`21264245686982643a88f2cee9183209b8b88cabfcca727591344eafe1d0e764`.
This remains local Git consumption, not public remote publication.

An earlier canonical-cell consumer pin is
`c27f99f79524fa60d5a7c38324cf65d9f47a5f18`, which additionally exercises
the public canonical int64 codec. The same 14-package/no-SQLite checks passed.
Its retained lockfile SHA-256 is
`c87557f211a63a3f801141eed04b11febea0c416dffa2a645f2f659a3a62c617`.
The query-result integration remains outside that earlier consumer's qualification.

The scoped-v4 consumer subsequently passed at
`8919ceaddf301d0c55aeeee12d4c62e3a2bb1edb`: all 16 product packages resolved to
that immutable local Git revision, all three HTTP compositions compiled, and
the graph contained no SQLx/SQLite. Its retained lock SHA-256 is
`e06f3a8696c459c078fc185627c18baf64c54f928160d335d3f401aef3697f19`.
This is not remote publication. The newly extracted analysis/execution libraries
were subsequently consumed at `22b7d7336c17d03832d7d52f8b47d4c3cddf683f`.
That separate local Git build passed with **19** uniformly pinned public crates,
no SQLx/SQLite or app dependency, and lock SHA-256
`1a932d9e50d43eccf1a4fe1cd5f759c0fe26a05151d65a764505e33b4e28940e`.
The later materialization/admission/canonical/runtime slice is covered by the
final consumer pin recorded above.

The first standalone run exposed a fixture helper's accidental reliance on
workspace-unified `serde_json/preserve_order`. The helper now deserializes the
typed fixture directly, retaining required schema discriminator order without
requiring consumers to enable unrelated features. The corrected pinned run
passed; the failed initial revision is not recorded as consumer-qualified.

On the local Windows environment, the focused store/service/shared-HTTP command
above passed **53 tests**, with **2 historical .NET differential tests ignored**
(not passes). The packaged-skill test passed separately. Focused all-targets
Clippy passed, and the shared crates also built without SQLite enabled.

The subsequent workspace/evidence-boundary slice passed **82 tests** across
workspace recovery, local Python analysis, evidence, inbox and codec packages.
All-targets Clippy passed for the changed local composition and adapters, and
contract-only checks passed with local defaults disabled. The initial local
analysis run lacked its documented Python environment; creating `.venv` and
installing the existing requirements resolved that prerequisite. A concurrent
build observed an intermediate feature-manifest/source mismatch during editing;
the complete rerun against the stable source passed. No live provider was used.

The canonical-cell codec additionally passed its shared Rust corpus and
JavaScript schema tests (two tests in each runner), including exact int64
boundaries, non-finite double payload bits and malformed encodings. Evidence
all-targets Clippy and SDK type-checking passed. This validates the scalar
contract; that older pin does not qualify the later query integration.

The integrated evidence-profile slice passed **76 Rust tests** across local
analysis, deterministic Tetris analysis, evidence and shared conformance,
plus **3 JavaScript schema tests** and **2 Python decoder tests**. The C11
conformance test executes the twelve required profile categories under two
independent scopes. Focused all-targets Clippy, SDK type-checking and the
contract-only evidence build passed. A query-drop regression additionally
verifies that cancellation interrupts backend work instead of leaving it
running until the original deadline.

The deterministic Tetris full-loop passed against the extracted runnable apps:
contract acceptance, SDK decisions, OTLP durable ingestion, evidence
materialization/restart, reviewed deterministic analysis, Candidate admission,
activation, SDK parity after activation and restart preservation. It reported
1,004 evidence observations, a successful reviewed Candidate and active state
version 3. This is deterministic local regression evidence, not live inference,
provider access, multi-host consistency, tenant isolation or hosted policy proof.
Node was absent from PATH; a session-local checksum-verified Node 22.23.5 and
the repository lockfile's missing npm dependencies were used for this check.
No machine-global runtime configuration or cloud resource was changed.

Issue #94 was subsequently completed by merged PR #95. The missed portable-policy
and contract-decoder boundary is corrected and qualified separately above;
that publication does not reopen or inherit approval from the historical PR.
Hosted workload/SLO, retention and distributed-adapter qualification remain
separate from these public contract/extraction tasks.
