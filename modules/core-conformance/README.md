# Shared adapter conformance

`flaggo-core-conformance` exports outcome-oriented scenarios against
`AuthorityFixture`, with no SQL dependency. Implement two independent clients
over one clean authority store and a failure control at publication, then invoke
each scenario on fresh storage. The functions panic on conformance violations.

| Entry point | Local coverage |
| --- | --- |
| `canonical::c01_canonical` | Canonical hashes, named collections, literal/rule order, scope identity, numeric boundaries and evaluator results |
| `runtime::c02_exact_runtime` | Exact version despite management-current changes, scope isolation, authority outage and persisted corruption with cached/fresh readers |
| `c07_namespace` | Independent full-resource ownership, catalogs and pagination for the same name |
| `c03_publication` | Same-version races, stable acceptance, active readiness and redeploy/current ordering |
| `c03_atomic_failure` | Failure after staging exposes no contract/current/active/artifact or name reservation |
| `c04_admission` | Independent-client idempotency, conflicting retry without orphan, stale retry/new admission rejection |
| `c05_activation` | Competing fenced activation, acknowledged active read, learned executable survives redeploy |
| `policy::c03_c05_policy` | Database-free bootstrap/redeploy, admission retries, manual/automatic tokens, ABA, rejection and all resolution outcomes |
| `policy::persisted_artifact_integrity` | Shared raw-artifact binding/digest/checked alignment and integrity-versus-native JSON errors |
| `policy::c05_adapter_policy_parity` | Shared manual/automatic plans match adapter-committed results and resolved retries |
| `evidence::c08_durable_acceptance` | Failed append has no receipt/effect, independent reads preserve payload/receipt, raw transport retries remain distinct |
| `evidence::c09_atomic_projection` | Epoch/checkpoint preconditions, stale-writer rejection, atomic failure, retries, observation dedup and content-conflict effects |
| `evidence::c11_negotiated_queries` | Profile selection, full scope/exact-or-null/source filtering, limits, typed results, projected-only IDs, live late arrivals and required profile corpus under two scopes |
| `admission::c04_c10_fenced_admission` | Released/replaced ownership, original-provenance recovery, idempotency and closed-cycle rejection |
| `workspace::c10_c12_fenced_publication` | Claim exclusion, immutable artifacts, conditional handoffs, execution receipt recovery and uncertain-execution non-replay |

`apps/contract-service/tests/authority_conformance.rs` supplies the SQLite
fixture and failure injection. Scenario assertions do not query physical tables.
Existing store tests additionally cover ABA, rollback and token exhaustion.
The [C01-C14 plan](../../docs/design/architecture/PORTABLE_CORE_CONFORMANCE.md)
is larger than these local suites: they do **not** certify C01-C14, independent
authentication, stale distributed workers, isolated execution or distribution.

`evidence::InboxFixture` and `EvidenceFixture` supply semantic capabilities,
fresh clients and injected append/checkpoint failures. `QueryCases` supplies
representative provider SQL and a fixed labelled `QueryCorpus`. Every profile
must include all twelve required C11 categories; provider-specific arithmetic
is explicit rather than assumed portable. The SQLite corpus is
`contracts/fixtures/evidence-query-sqlite-v1.json`. Shared assertions contain
no table or connection assumptions; only `tests/sqlite_evidence.rs` implements
SQLite fault controls. Existing SQLite tests additionally verify real backend
deadline/cancellation interruption and connection reuse.

```powershell
cargo test -p flaggo-contract-service --test authority_conformance --locked
cargo test -p flaggo-contract-service --test admission_fencing --locked
cargo test -p flaggo-decision-service --test runtime_conformance --locked
cargo test -p flaggo-core-conformance --test sqlite_evidence --locked
```
