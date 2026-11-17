# Shared-core extraction: authority and HTTP slice

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
| Workspace/execution | Existing local filesystem/lock/Python behavior, **not** a durable remote execution protocol or sandbox |
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
| C07 | Scoped routes, resource keys, independent authentication/security/error artifacts and SDK cutover not implemented; a concrete reviewed API revision is still required |
| C08-C09 | Existing inbox/evidence tests retained, but their outcome suites are not yet extracted; authorized whole-batch envelope and fenced projection protocol remain outstanding |
| C10 | Durable monotonic worker fences and authoritative stale-write rejection are not implemented |
| C11 | Versioned capability negotiation, lossless result schema/profile and required query corpus are not implemented |
| C12 | Local recovery behavior retained, but portable workflow/artifact/execution contracts still expose local filesystem assumptions and need extraction |
| C13-C14 | Hosted isolation, operational envelope, migration/restore/data-lifecycle qualification remain later gates; no numeric policy inferred |
| Consumer/publication | Script and local build mechanism provided; exact execution evidence and public remote reachability must be recorded independently |

Thus #94 must remain open. The next protocol work needs reviewed concrete
scoped API/security, evidence-result/profile and artifact/execution capability
artifacts before claiming those capabilities. The deferred hosted workload/SLO
and retention policies do not block these public contract/extraction tasks.
