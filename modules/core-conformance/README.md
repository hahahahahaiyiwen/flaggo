# Shared adapter conformance

`flaggo-core-conformance` exports outcome-oriented scenarios against
`AuthorityFixture`, with no SQL dependency. Implement two independent clients
over one clean authority store and a failure control at publication, then invoke
each scenario on fresh storage. The functions panic on conformance violations.

| Entry point | Local coverage |
| --- | --- |
| `c03_publication` | Same-version races, stable acceptance, active readiness, redeploy/current ordering, v3 ownership |
| `c03_atomic_failure` | Failure after staging exposes no contract/current/active/artifact or name reservation |
| `c04_admission` | Independent-client idempotency, conflicting retry without orphan, stale retry/new admission rejection |
| `c05_activation` | Competing fenced activation, acknowledged active read, learned executable survives redeploy |
| `evidence::c08_durable_acceptance` | Failed append has no receipt/effect, independent reads preserve payload/receipt, raw transport retries remain distinct |
| `evidence::c09_atomic_projection` | Checkpoint failure leaves no projection, retries reconcile, observation dedup and content-conflict effects |
| `evidence::c11_negotiated_queries` | Profile selection, full scope/exact-or-null/source filtering, limits, typed results, projected-only IDs, live late arrivals and required profile corpus under two scopes |

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
cargo test -p flaggo-core-conformance --test sqlite_evidence --locked
```
