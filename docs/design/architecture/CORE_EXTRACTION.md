# Shared-core extraction: authority, HTTP and workspace boundaries

Status: **partial implementation of #94**, not closure or hosted qualification.
Based on the approved architecture revision
`83088e825a0380a2a629b3bdd37cf8ffaaf1ae80`. The accepted design models in
[PORTABLE_CORE](PORTABLE_CORE.md) are not implemented wire revisions.

## Implemented ownership and adapter contract

| Component | Responsibility |
| --- | --- |
| `flaggo-contract-application` | Shared validation, compilation, management HTTP/projections, Candidate admission and activation worker |
| `flaggo-decision-http` | Shared runtime HTTP, telemetry and injected readiness; uses the existing `DecisionRuntime` |
| `flaggo-contract-store`, without default features | Accepted-version types, errors, timestamp/cursor contracts |
| `flaggo-executable-store`, without default features | Lifecycle types/errors and `ContractAuthority` semantic capability |
| `SqliteContractAuthority` | Local atomic authority adapter over one shared SQLite pool |
| Existing runnable apps | Local configuration, adapter initialization, listener and shutdown composition |
| `flaggo-core-conformance` | Reusable authority outcome scenarios, independent of SQL |
| `flaggo-analysis-workspace`, without default features | Path-free workflow and virtual filesystem contracts; local adapter remains feature-gated |
| `flaggo-evidence-store` and `flaggo-raw-otlp-inbox`, without default features | Evidence/query and durable inbox contracts without SQLx or SQLite query-parser dependencies; existing local adapters remain defaults |

`ContractAuthority` v1 describes the **current global-name v3** resource model.
It is not the future full-scope identity/auth capability. Reads and mutations
must share one authority domain. `publish_ready` commits name ownership,
immutable contract and bootstrap artifacts, activation and management current
together. `admit_current` checks currentness and retries inside the same
transaction as artifact/provenance insertion. Candidate/active state-token
activation and automatic resolution retain their existing semantic outcomes.
Integer exhaustion fails rather than wrapping lifecycle tokens.

SQLite shares the existing persistence and integrity routines inside those
transactions; the app no longer sequences separate writes or uses a
process-local current/admission mutex. A failed operation is not a claim that a
remote commit rolled back: future adapters must preserve explicit dependency
errors and reconciliation guarantees. No remote retry protocol is certified here.

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
local convenience without pretending that filesystem rename is a portable
conditional artifact publication protocol.

The evidence and raw-inbox crates likewise separate their contracts from
optional local adapters; the OTLP codec no longer enables SQLite transitively.
Their existing semantics, query envelope and local limits are unchanged. This
is dependency extraction, not new query negotiation, authorized ingestion or
distributed projection fencing.

## Approved next wire cutover

After reviewing this slice, the user selected
**"Use v4 for the full-scope API (Recommended)"** and
**"Replace v3 with an explicit breaking v4 cutover (Recommended)"**.
The next route is therefore:

```text
/v4/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}
```

These are decisions for the next implementation, not a claim that v4 endpoints,
schemas, SDKs or migrations exist. Current runnable apps still serve v3.
The cutover must preserve unchanged-content digests and historical provenance,
align wire/SDK/fixtures/local resource keys, and document migration explicitly;
it must not silently reinterpret v3 names.

## Compatibility record

| Surface | This slice |
| --- | --- |
| Product revision | The Git commit containing this record; consumer runs additionally record their exact immutable source SHA in `result.json` |
| Public HTTP and SDK | Management/runtime **v3**, unchanged routes, bodies and generated SDK artifacts |
| Expression/canonical identity | `flaggo.cel/v1`; unchanged contract/executable digest algorithms and fixtures |
| SQLite schemas | Contract **3**, executable **5**, evidence **4**, raw inbox **1**; no reset or new migration |
| Authority capability | `flaggo.authority/v1`: globally name-keyed atomic ready publication/current admission and existing activation/read contracts |
| Reusable fixture revision | `flaggo.authority-conformance/v1`, local subsets of C03-C05 |
| Evidence query | Existing SQLite dialect/type envelope, **not** the approved future portable profile |
| Workspace/execution | Path-free lease contract plus explicit local projection; existing local filesystem/lock/Python behavior, **not** a durable remote execution protocol or sandbox |
| First hosted slice | Not qualified: scoped API/security revision and private distributed adapter still required |

Concurrency strengthens persistence without changing PUT retry semantics:
identical redeployment preserves acceptance identity and learned activation;
successful publication selects current, including an old-version redeploy.
Admission still checks currentness **before** duplicate reconciliation, returning
the existing v3 stale-contract response. Activation tokens, conflict mapping,
catalog ETags, pagination and exact runtime lookup retain existing contracts.
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

Focused checks:

```powershell
cargo test -p flaggo-contract-service -p flaggo-decision-service -p flaggo-contract-store -p flaggo-executable-store -p flaggo-decision-http --locked
cargo check -p flaggo-contract-application -p flaggo-decision-http --locked
cargo clippy -p flaggo-contract-application -p flaggo-decision-http -p flaggo-core-conformance --all-targets --locked -- -D warnings
cargo test -p flaggo-async-analysis packaged_skills_override --locked
cargo test -p flaggo-analysis-workspace -p flaggo-async-analysis -p flaggo-evidence-store -p flaggo-raw-otlp-inbox -p flaggo-otlp-codec --locked
cargo check -p flaggo-analysis-agent -p flaggo-analysis-workspace -p flaggo-evidence-store -p flaggo-raw-otlp-inbox -p flaggo-otlp-codec --no-default-features --locked
npm run test:tetris-full-loop
```

The [separate consumer](../../../tools/core-consumer/README.md) compiles both HTTP
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

## Remaining acceptance gaps

| Gate | Actual state |
| --- | --- |
| C01-C02 | Existing domain/compiler/runtime tests retained; injected-reader HTTP cache/failure test added. Full reusable cross-adapter corpus remains incomplete |
| C03-C05 | Reusable local publication/failure/admission/activation outcomes added; existing store ABA/rollback coverage retained. Multi-process barrier/failure matrix and actual distributed proof remain outstanding |
| C06 | Local injected-reader behavior only; multi-host acknowledgement/outage qualification remains phase 3 |
| C07 | V4 and explicit breaking cutover approved above; scoped routes, resource keys, independent authentication/security/error artifacts, SDK and migration are not implemented |
| C08-C09 | Existing inbox/evidence tests retained, but their outcome suites are not yet extracted; authorized whole-batch envelope and fenced projection protocol remain outstanding |
| C10 | Durable monotonic worker fences and authoritative stale-write rejection are not implemented |
| C11 | Versioned capability negotiation, lossless result schema/profile and required query corpus are not implemented |
| C12 | Host paths removed from the lease contract and exact-claim local projection tested; durable conditional publication/fence/execution receipts and reusable recovery scenarios remain outstanding |
| C13-C14 | Hosted isolation, operational envelope, migration/restore/data-lifecycle qualification remain later gates; no numeric policy inferred |
| Consumer/publication | Local immutable consumer verified below with a retained lock; public remote reachability remains unverified |

## Observed local verification

The source pinned by the successful standalone consumer is
`ef2541081231a2e12c738731a6bcf14bbf8d33c3` (authority/HTTP extraction plus its
consumer-fixture correction). The build used an explicit local Git URL, not a
remote branch or a path dependency. Its retained `Cargo.lock` SHA-256 is
`c3f39fd7fd2b2d255ecd1cc00e16cbc40fc6e4b890d93d22e378093600b069df`.
All ten transitive product crates resolved to that same Git revision; metadata
contained no SQLx/SQLite package. Both HTTP compositions compiled, the canonical
fixture digest matched, and exact-contract evaluation returned the expected
default. Public remote reachability has **not** been tested or claimed.

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

Thus #94 must remain open. The next protocol work needs reviewed concrete
scoped API/security, evidence-result/profile and artifact/execution capability
artifacts before claiming those capabilities. The deferred hosted workload/SLO
and retention policies do not block these public contract/extraction tasks.
