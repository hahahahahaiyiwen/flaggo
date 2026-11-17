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

`apps/contract-service/tests/authority_conformance.rs` supplies the SQLite
fixture and failure injection. Scenario assertions do not query physical tables.
Existing store tests additionally cover ABA, rollback and token exhaustion.
The [C01-C14 plan](../../docs/design/architecture/PORTABLE_CORE_CONFORMANCE.md)
is larger than this first reusable suite: these functions do **not** certify
C01-C14, scoped identities, authentication, evidence, execution or distribution.

```powershell
cargo test -p flaggo-contract-service --test authority_conformance --locked
```
