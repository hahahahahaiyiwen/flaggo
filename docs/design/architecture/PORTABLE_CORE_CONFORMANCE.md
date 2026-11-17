# Portable core compatibility and conformance plan

## Status

Phase-1 plan for [#93](https://github.com/hahahahahaiyiwen/flaggo/issues/93).
Read with the [ADR and decision register](PORTABLE_CORE.md) and
[behavior decisions and proposals](DISTRIBUTED_BEHAVIOR.md). This is a specification of
required evidence, not a test execution report or proof of hosted readiness.
Pending operational policies block their affected hosted scenarios, not the
narrowed #93 documentation gate. Approved behavior models remain unchanged.

## Compatibility matrix

| Surface | Current local behavior | Intended shared/hosted behavior | Compatibility class and gate |
| --- | --- | --- | --- |
| Digest and evaluator | Authority-bound canonical contracts; deterministic bounded evaluator | Same implementation and fixtures | Unchanged for unchanged content; D1 must not silently alter hashes |
| Exact decisions | v3 exact name/digest; no current-version lookup, persistence or SDK fallback | Preserve stateless evaluation | Unchanged semantics; proposed scoped route requires versioned wire change |
| Contract resource identity | Global name ownership by one full authority | Approved full authority plus name | Intentional versioned API/resource-key change, D1 |
| Authentication/errors | No implemented server auth boundary; declarations are not credentials | Independently authenticated, operation/scope authorization | Hosted capability plus explicit versioned security/error contract, D2; not drop-in v3 equivalence |
| Catalog/history | Global current catalog, name-keyed history | Authorized inventory, scope-bound pagination/ETag | Intentional visibility change, D1/D2 |
| Activation | Atomic SQLite replacement, current/candidate/active token checks | Same shared policy with authoritative reads across replicas | Required capability guarantee, D3; stale activation cache unsupported |
| Acceptance/admission | Staged store calls and process-local admission/current mutex | Atomic accepted-ready publication and transactional admission checks | Infrastructure behavior strengthening, D3; retain idempotency and ready response shape |
| Decision retries | Reuse complete input; active executable may change between calls | Same | No response replay guarantee introduced |
| Candidate retries | Workspace/cycle identity and content checks; currentness checked before duplicate lookup | Durable identity/reconciliation and fences | Preserve or explicitly version changed retry outcomes, D4 |
| OTLP | Whole-request append before acknowledgement; unauthenticated multi-resource transport | Durable authenticated envelope and pre-append scope check | OTLP payload remains standard; profile/security/admission changes, D2/D4 |
| Projection | Atomic SQLite observations/provenance/conflicts/diagnostics/checkpoint; one active materializer | Same logical effects with checkpoint preconditions and fenced ownership | Capability extension, D4; no automatic replay on version change |
| Analysis SQL | SQLite dialect/types, live per-query scope, allowlisted relation/functions | Negotiated versioned profile and canonical result semantics | Query/result contract change, D5; arbitrary SQL not portable |
| Workspace | Local paths, filesystem projection, file locks | Scoped durable handles, conditional state/artifact publication | Local convenience retained; portable port change, D4/D7 |
| Python execution | Trusted host process, inherited environment | Isolated bounded hosted execution with scoped inputs/outputs | Local convenience versus hosted-only operational guarantee, D7 |
| Capacity/retention/recovery | Local configured defaults; no hosted SLO proof | Explicit approved limits, policy and verified restore | Separate D6/D7 follow-up gates; no numeric target or retention policy approved by #93 |
| Distribution | Public local libraries/apps | Same pinned core with private composition/adapters | No private dependency in public builds; phase-2 consumer-build gate |

Versioned changes require a dedicated accepted contract revision with aligned
schemas, SDK generation, fixtures, error mappings and deployment/runtime
configuration. Do not mix scoped and global lookup semantics under a single
unqualified endpoint. Migration must preserve existing immutable digests,
authority and artifact identities; database format/version acceptance remains
explicit. Rollout/restore procedures belong to the deployment owner.

## Shared test contract

Phase 2 should extract outcome-oriented test functions/fixtures usable by local
and private adapters, not copy SQLite assertions. Each adapter supplies a
fixture factory, clean scoped storage and failure/barrier controls. Test code
must not require public contributors to access private packages or credentials.

Record the public revision, wire/behavior/capability revisions, adapter revision,
fixture revision, configuration, commands and observed results. Separate
supported, unsupported, skipped and failed capabilities. A skipped distributed
test cannot count as a pass. Private qualification publishes sanitized results
against the public case IDs without disclosing private implementation.

| ID / gate | Setup and action | Required assertions | Existing prior art to reuse |
| --- | --- | --- | --- |
| C01 / phase 2 | Run canonical fixtures with reordered named collections, description-only changes, numeric boundaries, authority changes and expression normalization | Identical canonical hashes/evaluation across consumers; changed semantic identity changes hash; malformed input fails identically | [Contract tests](../../../modules/contract/tests/domain.rs), [compiler tests](../../../modules/expression/tests/compiler.rs), [wire conformance](../../../contracts/conformance/validate.py) |
| C02 / phase 2 | Evaluate same contract/input/checked executable in local and consumer compositions; alter current pointer only; corrupt cached/stored artifact | Exact digest remains selected; parity of result/errors; integrity failures never become fallback | [Runtime tests](../../../modules/decision/src/runtime/tests.rs), [Decision HTTP tests](../../../apps/decision-service/tests/http.rs) |
| C03 / phases 2-3 | Synchronize concurrent same/different-version deploys and ownership claims; fail between staging/bootstrap/current publication; redeploy after learned activation | One authorized owner, stable accepted identity, no ready-without-active result; publication ordering explicit; learned activation not reset | [Contract store tests](../../../modules/contract-store/tests/store.rs), [Contract HTTP tests](../../../apps/contract-service/tests/http.rs) |
| C04 / phases 2-3 | Pause admission at currentness check, publish newer contract through another client, then release; race identical/conflicting cycle submissions | No stale newly admitted candidate after fence; one admission for same identity/content; conflict leaks no artifact | [Executable store tests](../../../modules/executable-store/tests/store.rs), current HTTP submission flow |
| C05 / phases 2-3 | Race activation with new current contract, another activation and A -> B -> A; inject failure after deactivation | One valid transition, stale token rejection, no torn state; rollback preserves prior active executable | `competing_manual_activations_have_one_winner_and_supersede_the_loser`, `automatic_resolution_detects_aba_and_concurrent_resolution_is_idempotent`, `failed_activation_rolls_back_deactivation_and_preserves_pending_candidate` in executable store tests |
| C06 / phase 3 | Cache executable on replica A, activate through B, then begin a decision on A after acknowledgement; interrupt authority reads | New or later committed activation selected; overlapping request may finish old program; outage returns failure, not stale-cache success | Runtime reader ports; new distributed harness required |
| C07 / phases 2-3 | Create same name under two full scopes; authenticate as each and probe exact/history/catalog/cursors/candidate/activation routes and scope mismatches | Namespace follows D1; no cross-scope data/mutations/existence leakage; errors follow D2; no claims-selected alias | Existing authority-conflict fixtures are baseline, not auth proof |
| C08 / phases 2/4 | Lose OTLP append response; retry; duplicate same observation and conflict its content; race capacity/retention; include one forbidden Resource | No acknowledgement before durable commit; explicit duplicate/conflict semantics; rejected batch leaves no writes; capacity is atomic | [Inbox tests](../../../modules/raw-otlp-inbox/tests/sqlite_inbox.rs), [materializer tests](../../../apps/evidence-materializer/tests/materializer_pipeline.rs) |
| C09 / phases 2/4 | Crash at each projection/checkpoint boundary; retry; expire inbox data ahead of consumer; change producer versions | Projection and checkpoint commit together; no duplicate effects, no silent gap, no implicit backfill | [Evidence store tests](../../../modules/evidence-store/tests/sqlite_store.rs) |
| C10 / phases 4-5 | Pause worker beyond lease expiry; replacement acquires next epoch and commits; resume stale worker; skew worker clocks | Every stale checkpoint/handoff/budget/outcome/submission commit rejected; authority uses its own clock/order; recovery preserves committed work | Local file-lock tests cover exclusion only; new fence harness required |
| C11 / phases 2/4 | Run each advertised query scenario under two scopes; request unadvertised SQL/functions/physical tables; test late arrivals, empty/aggregate results, limit/cancel/reuse | Correct scope/source/exact-or-null digest filtering; explicit unsupported/limit failures; lossless types; truthful truncation and provenance; no leaked connection work | [SQLite analysis tests](../../../modules/evidence-store/tests/sqlite_analysis.rs) |
| C12 / phases 2/5 | Interrupt artifact upload and handoff publication; recover with orphan/newer files; replay candidate receipt after lost response | Only committed pointer advances workflow; content hashes bind artifacts; no duplicate candidate or guessed state | [Workspace tests](../../../modules/analysis-workspace/src/local.rs), [analysis pipeline tests](../../../apps/async-analysis/tests/analysis_pipeline.rs) |
| C13 / phase 5 | Run deterministic proposal/review/submission across isolated attempts; attempt cross-tenant artifact read, credential access, egress and resource exhaustion | Denial and enforced limits; no direct activation permission; recoverable attempts and valid reviewed candidate | Local deterministic full loop proves orchestration, not isolation |
| C14 / phase 6 | Exercise approved load/failure envelope, restore committed state/artifacts, migrate supported versions and test deletion/tombstone restore behavior | Measured D6 objectives; D7 retention/isolation; no dangling authority/artifact or resurrected forbidden data | New actual-target qualification; historical single-host diagnostics are insufficient |

For C11, fix a minimum profile corpus before claiming compatibility: source and
time filtering, explicit ordered projection, grouped count/aggregate, JSON
extraction, non-recursive CTE, empty result with types, int64 extremes,
numeric-looking strings, finite/non-finite doubles, NULL/missing JSON,
division/overflow/collation and duplicate aliases. Compare common semantics,
not unordered database row order. Provider-specific operations have separate
labelled fixtures, never a silent downgrade of the required corpus.

## What existing checks establish

The repository already has offline schema/fixture validation, Rust domain/store/
HTTP tests, SDK conformance and deterministic local end-to-end flows. They are
valuable regression gates, not evidence of multi-host security or consistency.
Useful existing entry points for phase-2 implementation:

```powershell
python contracts\conformance\validate.py
cargo test -p flaggo-contract-store -p flaggo-executable-store --locked
cargo test -p flaggo-evidence-store -p flaggo-raw-otlp-inbox --locked
npm run check:generated
npm run check:openapi
npm run test:tetris-full-loop
```

Use the smallest affected gate during implementation; follow the existing
development setup for dependencies. This documentation change does not add
tests, assert those commands were run, or claim a private adapter exists.

## Distributed-only acceptance

Phases 3-6 require the actual private composition: two isolated tenants,
multiple API replicas on separate hosts, independent worker processes, real
failure injection and durable storage. In-memory barriers or multiple
connections in one process can establish a shared contract test but cannot
prove that a process-local mutex is absent from correctness.

Repeat runtime isolation checks while evidence/analysis are stopped, slow and
resource-constrained. Independence means observed decision availability under
the approved failure/resource envelope, not merely absence of a code reference.
Record exact artifacts and commands. Deterministic analysis qualification
must be labelled deterministic; live inference acceptance is separate work.

## Issue acceptance ledger

This ledger assesses the narrowed architecture/documentation scope. "Covered"
means the repository document contains the required decision or plan; it does
not mean the implementation scenarios passed or that the issue is closed.

| Narrowed #93 acceptance | Repository evidence | Coverage / later gate |
| --- | --- | --- |
| Public ownership/extension ADR | [Owners and dependency rule](PORTABLE_CORE.md#approved-direction-owners-and-dependency-rule), contract layers and implementation-boundary evidence | Covered; no private repository access required |
| Approved naming/auth/activation/fencing/query models | [D1-D5 decision register](PORTABLE_CORE.md#decision-register) and dated approvals | Covered; concrete versioned artifacts and protocols remain #94/hosted implementation gates |
| Compatibility matrix | Matrix above | Covered; distinguish current wire behavior, intentional changes, local conveniences and hosted capabilities |
| Shared conformance plan and distributed-only separation | C01-C14, scenario corpus, evidence requirements and distributed-only section | Covered as a plan; implementation and execution belong to the owning later phases |
| Local execution/locks not hosted isolation | ADR implementation table and [execution boundary](DISTRIBUTED_BEHAVIOR.md#durable-artifacts-versus-execution-d4d7-proposal) | Covered; local locks and trusted Python are not hosted guarantees |
| Threat-model baseline and enforceable-resource-limit obligations | ADR D7 approval and execution-boundary obligations | Covered at architecture level; numeric budgets, concrete controls and actual-target proof remain hosted gates |
| Deferred workload/SLO and retention/deletion tracked separately | [Hosted operating envelope and owners](PORTABLE_CORE.md#d6-d7-required-hosted-operating-envelope); [hahahahahaiyiwen/flaggo-cloud#5](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/5) and [hahahahahaiyiwen/flaggo-cloud#6](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/6) | Covered as unresolved tracking with public summaries and hosted gate consequences; policy values are not approved or completed |

No missing architecture-model approval is identified in D1-D5 or the D7
baseline. Remaining review is to confirm this documentation coverage and
linked follow-up/dependency tracking against the live narrowed issue, and
verify durable public access to the document revision. A local commit or
in-app attachment is not public-documentation acceptance evidence. Record an
immutable public source revision or equivalent durable public document link
in #93 before closure. The maintainer/coordinator owns that acceptance and
closure; this ledger does not close the issue or imply a push or merge.

Deferred operating targets and retention/deletion must not be checked off as
completed merely because #93 can close. Phase 1 produces agreed architecture
contracts and a qualification plan; it neither delivers refactoring nor
satisfies hosted deployment acceptance.
