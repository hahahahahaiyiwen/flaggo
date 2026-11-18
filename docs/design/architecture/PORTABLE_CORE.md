# ADR: One portable product core

## Status and scope

The dated baseline below belongs to the #93 design gate. Implemented revisions
and remaining #94 acceptance are tracked separately in [CORE_EXTRACTION](CORE_EXTRACTION.md).

Phase-1 design for [#93](https://github.com/hahahahahaiyiwen/flaggo/issues/93),
under [#85](https://github.com/hahahahahaiyiwen/flaggo/issues/85).
The repository ownership direction below is **approved direction**, recorded
in those issues. D1-D5 and D7's threat-model baseline are approved below;
numeric operating targets, retention and remaining policy details are
**pending, not guarantees**.
This ADR does not change the current wire contracts or implement hosted behavior.

**Scope separation approved 2026-11-17:** #93 covers the approved architecture
and behavioral models, compatibility boundaries and conformance plan.
Workload/SLO targets and retention/deletion policy are separate follow-ups
blocking the relevant hosted gates, not completion of this documentation gate.
The approved threat-model baseline and the obligation to enforce resource
limits remain part of the architecture; choosing numeric limits and qualifying
concrete isolation controls remain hosted work.

Implementation evidence was inspected at public revision
`c397329f608c04631d309f2cfff966a58d80fe2c`. "Current" below means that revision,
not the historical backend-preparation branch or a deployed hosted service.
Phase 2, [#94](https://github.com/hahahahahaiyiwen/flaggo/issues/94), owns code
extraction and selective reconciliation of historical product fixes. Do not
merge the historical preparation branch wholesale or rewrite release identities.

The existing [application boundaries](APP_BOUNDARIES.md),
[authority model](AUTHORITY.md), [primitive semantics](../contracts/CONTRACTS.md),
and [executable wire contracts](../../../contracts/README.md) remain canonical
for implemented behavior. Approved future changes must update those sources and
their executable artifacts together; this proposal does not supersede them.

## Context

Sharing only DTOs would leave local and hosted services free to disagree about
validation, acceptance, candidate eligibility, activation, evidence and recovery.
Conversely, exposing existing SQLite methods or filesystem paths through traits
does not make their transaction or execution assumptions portable.

The reusable boundary is product behavior plus the capabilities that make its
invariants true. A storage port describes an atomic outcome, not arbitrary
table access. A runtime port describes scoped execution, not a host directory.
Local and hosted compositions use the same behavior implementation.

## Approved direction: owners and dependency rule

| Owner | Owns | Dependency boundary |
| --- | --- | --- |
| Public `flaggo` | Wire/behavior/capability contracts, reusable application/domain logic, compiler/evaluator, SDKs, shared conformance suites, local apps and adapters | Builds and runs without private packages, cloud credentials or hosted service access |
| Private `flaggo-cloud` | Hosted composition, distributed adapters/coordination, tenant provisioning, identity integration, authorization enforcement, quotas, isolated analysis execution | Consumes pinned public libraries; must not copy service rules or fork their semantics |
| `flaggo-deploy` | Infrastructure, release rollout, migration/recovery and operational qualification | Deploys identified artifacts; does not redefine product semantics |
| `flaggo-website` | Presentation and selected published documentation | Links to or publishes selected public canonical docs; is not a second semantics authority |

Public application/domain code depends on public module-owned capability
contracts. Local and private adapters implement them; application composition
roots supply adapters and policy. Public code never imports private code.
Logical store ownership does not require separate physical databases.

Public consumption initially uses immutable Git revisions and locked
dependencies, with a separate consumer build in phase 2. Publishing packages is
optional. API/schema, behavior/capability, product revision, adapter revision,
and deployment identity must remain separately identifiable.

The release gates remain: (1) contracts, (2) core extraction, (3) two-tenant
distributed authority and decisions, (4) distributed evidence, (5) isolated
learning, (6) release readiness. Hosted single-node is not a required release.
First-class local runnable support is retained at every gate.

## Contract layers

| Layer | Public authority | Extension rule |
| --- | --- | --- |
| Wire | OpenAPI, JSON Schemas, SDK types/validators, RFC 9457 errors, OTLP and service-observability profiles | Change versioned artifacts, fixtures and consumers together; auth is not an undocumented lookup parameter |
| Behavior | Canonicalization/digests, validation, compilation/evaluation, acceptance readiness, candidate admission and activation policy, evidence identities and workflow transitions | One shared implementation; adapters cannot redefine ordering, eligibility or error outcomes |
| Infrastructure capability | Atomic authority transitions, coherent readers, durable inbox commits, projection/checkpoint commits, fenced claims, scoped evidence queries, durable artifacts and execution | Document required consistency, retry, cancellation and failure guarantees; fail unsupported capabilities explicitly |
| Composition policy | Local settings or hosted identity, limits, retention and isolation enforcement | May restrict capabilities, never silently weaken their correctness guarantees |

Infrastructure errors retain distinct invalid-input, conflict, unavailable,
timeout, cancellation and integrity outcomes. A provider outage cannot become
"not found", an empty evidence result, a default decision, or successful work.
Physical schemas and SDK-specific error types stay in adapters.

## Current implementation and extraction boundaries

| Evidence | Implemented behavior | Phase-2 seam, not an implemented capability |
| --- | --- | --- |
| [Decision runtime](../../../modules/decision/src/runtime.rs), `ContractVersionReader`, `ActiveExecutableReader`, `DecisionRuntime` | Generic readers; validates exact identity/input, reads active authority per decision and caches compiled immutable programs | Reuse these reader ports and shared evaluator; retain error distinctions and add approved scope handling around them |
| [Decision HTTP](../../../apps/decision-service/src/lib.rs), `AppState`, `router`, `readiness` | HTTP state, runtime instantiation and probes use concrete SQLite stores | Shared HTTP/application behavior with injected readers and readiness capabilities; no separate private copy |
| [Contract lifecycle](../../../apps/contract-service/src/lifecycle.rs), `deploy`, `submit` | Concrete SQLite stores; deployment stages persistence/bootstrap/current publication; process-local `current_mutation` guards submission/current updates | Shared lifecycle orchestrates semantic authority commands; a mutex is not a multi-replica admission fence |
| [Executable lifecycle](../../../modules/executable-store/src/lifecycle.rs), `activate_candidate`, `resolve` | Transaction checks contract-current, candidate and active state versions; updates active state and supersedes candidates together | Preserve atomic cross-record outcomes; moving contract and executable state into separate stores must not split this transaction |
| [Contract store](../../../modules/contract-store/src/lib.rs), `put`, `set_current` | Global name ownership, immutable authority-bound versions and separate current-pointer publication | Accepted-ready publication and concurrency semantics must be explicit, not inferred from generic CRUD |
| [Evidence store](../../../modules/evidence-store/src/lib.rs), `EvidenceStore::commit` | Semantic projection commit includes observations and diagnostics; SQLite also persists deduplication/provenance/conflicts and checkpoint atomically | Preserve transaction scope; add approved checkpoint preconditions and worker fencing |
| [Evidence query](../../../modules/evidence-store/src/analysis.rs), [SQLite query adapter](../../../modules/evidence-store/src/sqlite_analysis.rs) | Capability discovery exists, but SQL dialect, declared types and query semantics are SQLite-oriented | Implement the approved D5 negotiation model and reviewed result/profile schemas before claiming second-adapter portability |
| [Workspace provider](../../../modules/analysis-workspace/src/provider.rs), [local implementation](../../../modules/analysis-workspace/src/local.rs) | Semantic cycle methods coexist with `PathBuf`, filesystem rename and local file-lock claims | Separate durable state/artifact publication from execution and local filesystem projection |
| [Local Python](../../../apps/async-analysis/src/python.rs), `PythonConfig::run` | Host process inherits host access/environment; timeout and output limits are not isolation | Retain trusted-local adapter; hosted execution is a separately qualified capability |
| [Materializer](../../../apps/evidence-materializer/src/materializer.rs), [analysis coordinator](../../../apps/async-analysis/src/coordinator.rs) | Product orchestration remains in app crates as well as modules | Extract reusable behavior where needed, not every function into a speculative plugin |

Current SQLite transactions already protect important races; the gap is not
"SQLite has no atomicity." The gap is preserving those guarantees across
replicas, stores and remote execution while removing infrastructure coupling.

## Decision register

Approval of the ownership direction alone does not approve any row below.
The decisions explicitly approved in this session are recorded separately.
No operational numbers are inferred from local defaults.

| ID | Choice and recommendation | Status / blocked work |
| --- | --- | --- |
| D1 | Explicit `(tenant, application, environment, contractName)` resource namespace in a versioned API; retain current digest algorithm for unchanged contract content | Approved direction, 2026-11-17; concrete API revision and migration remain phase-2 work |
| D2 | Authenticate independently of declarations; authorize explicit resource scope and operation, conceal inaccessible resource existence, scope all catalogs/evidence/artifacts; authorize every OTLP Resource before acknowledging the whole batch | Approved 2026-11-17; versioned security/profile artifacts and implementation remain required |
| D3 | Authoritative transactional/version-fenced activation reads; immutable artifact caching only; atomic accepted-ready publication and fenced admission | Consistency model approved 2026-11-17; implement and qualify in phases 2-3 |
| D4 | At-least-once attempts with durable operation identities, atomic commits and storage-enforced monotonically increasing worker fences; OTLP observation-level deduplication | Guarantee model approved 2026-11-17; concrete protocols and retention windows still need specification/qualification |
| D5 | Versioned query capability negotiation with common scoped relations/result semantics and required fixture coverage; no promise of identical arbitrary SQL | Portability model approved 2026-11-17; concrete result/profile schemas and minimum corpus must precede adapter claims |
| D6 | Choose measured initial workload and numeric availability/latency/freshness/recovery targets before platform selection | Unresolved in [hahahahahaiyiwen/flaggo-cloud#5](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/5), outside #93; capacity qualification and target-dependent platform selection remain blocked |
| D7 | Hostile-tenant/untrusted-execution isolation, data lifecycle and enforceable per-scope limits | Threat-model baseline and enforcement obligations approved; retention/deletion unresolved in [hahahahahaiyiwen/flaggo-cloud#6](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/6); numeric limits and concrete controls remain hosted gates |

### D1: naming and digest compatibility

Today names are globally owned by their first declared `AuthorityScope`.
Exact reads use `(contractName, contractDigest)` and current reads use the name;
credentials do not select a hidden namespace. Authority is already included
in the contract digest. The public API currently has no authentication boundary.

**Approved 2026-11-17:** in the phase-1 design session, the maintainer selected
"Full tenant/application/environment/name namespace (Recommended)" after being
shown the versioned-route/SDK and unchanged-content digest implications.
This approves the namespace direction, not a particular API version or rollout.

Make the full authority explicit in resource identity and routing.
This allows the same logical contract name in different tenants, applications
and environments without renaming authored contracts. Tenant-only names still
collide across an application's development/production environments.
Illustrative route, not a reserved API version:

```text
/<next-api-version>/tenants/{tenant}/applications/{application}/environments/{environment}/decision-contracts/{contractName}
```

Version/history/decision routes extend that explicit resource. The route scope
must match the contract's declared authority; no header, token claim, default
environment or guessed tenant rewrites it. Reject mismatches.

Alternatives: retain global names (least API change, global collisions and name
reservation burden); use tenant-only names (fewer route fields, still collides
across application/environment); credential-selected names (rejected
recommendation: changes the same URL's identity invisibly).

Changing the lookup key alone need not change existing digests: the current
canonical contract already contains name and full authority. Changing name or
authority still changes the digest. Preserve canonicalization and expression
profiles; do not strip authority, salt hashes with credentials or rehash old data.
Existing records can be indexed by their stored scope after integrity checks.
Do not silently enable duplicate scoped names under v3: publish aligned
OpenAPI/schema/SDK/configuration changes and an explicit cutover policy first.
No obsolete-format compatibility layer is proposed. D1 approval does not alone
approve a route version or rollout/migration implementation.

### D2-D5: approved models and proposed protocol details

The [distributed behavior design](DISTRIBUTED_BEHAVIOR.md) records separate
approvals for authorization, atomic authority, worker and query models, their
current counterparts and proposed protocol details. Approval of a model does
not make its implementation complete or approve unspecified wire revisions.

### D6-D7: required hosted operating envelope

The threat-model baseline is approved below. The remaining fields are
**unfilled approval inputs**, not promised service levels or additional #93
documentation acceptance criteria:

| Decision input | Required specification | Accountable owner |
| --- | --- | --- |
| Workload | Initial/peak tenants, applications/environments, contract/version counts and bytes, decision RPS/concurrency/input size, activation rate | Product maintainer with hosted owner |
| Evidence/analysis load | OTLP bytes/s, burst duration, resource scopes/request, cardinality, evidence query concurrency/scan bounds, analysis attempts and execution budget | Hosted owner with product maintainer |
| Service objectives | Availability window/exclusions; decision/management latency percentiles at stated load; evidence freshness and analysis scheduling delay separately | Product maintainer |
| Failure envelope | Host/zone loss and partition assumptions; acknowledgement durability, RPO/RTO, backup and restore verification scope | Hosted and deployment owners |
| Data lifecycle | Raw inbox, evidence, query results, workspace artifacts, admission/dedup records, audit and backups: retention, deletion deadline and restore/tombstone behavior | Product maintainer with deployment owner |
| Isolation | Malicious tenant, forged telemetry, compromised execution, noisy neighbor; trusted operator/control-plane assumptions and accepted residual risks | Product maintainer with hosted owner |
| Limits | Per-scope requests/bytes, storage, query CPU/time/rows/bytes, execution CPU/memory/time/output/network, concurrency and provider budget | Hosted owner; product approval of externally visible behavior |

Recommendation: qualify the two-tenant/multiple-host authority slice before
evidence or AI; derive capacity targets from an approved workload rather than
turning the two-tenant correctness minimum into a capacity target. Treat tenant
content and generated Python as untrusted in hosted mode; prohibit execution
from obtaining control-plane/store credentials or another tenant's artifacts.

**Threat-model baseline approved 2026-11-17:** the maintainer selected "Approve
malicious-tenant and untrusted-execution baseline (Recommended)" after reviewing
cross-tenant/control-plane protection, explicit network/secret restrictions
and resource limits. Trusted operators/control plane are outside this initial
attacker model. This does not claim that their compromise is mitigated.
The exact sandbox, network allowance, secret access, numeric limits, retention
values and supported failure domain still require approval and qualification.

**Explicit deferrals, 2026-11-17:** the maintainer selected "Leave workload/SLO
values pending; do not invent targets" and "Leave retention/deletion policy
pending; do not reuse local defaults as hosted policy". Approved shared-core
semantics may guide extraction. The subsequent approved scope separation moves
these unresolved policies out of #93 while retaining their capacity,
hosted data-lifecycle and platform-selection blockers.

The follow-up boundaries are:

| Follow-up | Must resolve | Gates that remain blocked |
| --- | --- | --- |
| Workload and service objectives: [hahahahahaiyiwen/flaggo-cloud#5](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/5) | Approved workload envelope, availability/latency/freshness/recovery objectives and resource-budget inputs | Target-dependent platform/capacity decisions; capacity and SLO acceptance for the affected hosted slices; final release qualification |
| Retention and deletion: [hahahahahaiyiwen/flaggo-cloud#6](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/6) | Policy by data class, retry/dedup recovery windows, deletion deadlines and backup/restore behavior | Hosted data-lifecycle acceptance, dependent evidence/learning retention and recovery behavior, final release qualification |

These are tracked design obligations, not completed decisions. Correctness
refactoring and conformance-fixture work can proceed without inventing target
values. Closing a documentation issue cannot waive the follow-up gates.
The hosted owner coordinates both follow-ups with the product maintainer and
deployment owner identified above. The linked implementation repository is
private; the categories, owners and gate consequences in this public table are
sufficient to understand the unresolved obligations without access to it.

Do not select Kubernetes, a broker, sharding, multi-region writes or per-service
databases without those inputs. A shared transactional authority store is a
simpler candidate than cross-store coordination, not a platform approval.
Retaining local support does not authorize exposing trusted-local endpoints or
Python execution to untrusted users.

## Consequences and implementation gates

There is one evaluator and one set of lifecycle policies to maintain. Adapters
must do more than implement matching Rust method signatures: they must satisfy
the [compatibility and conformance plan](PORTABLE_CORE_CONFORMANCE.md).
Some existing ports need revision; this ADR does not claim extraction is done.

Phase 2 may extract shared behavior and local adapters on the approved D1-D5
models. Scoped routes/security, result schemas and capability protocols still
need reviewed executable artifacts before implementations claim conformance.
The narrowed #93 gate can be accepted once its documentation criteria and
explicit follow-up tracking are satisfied; D6 and the deferred parts of D7 do
not block that acceptance. They continue to block the hosted gates above.
Hosted implementation is outside #93 and #94.

The [acceptance ledger](PORTABLE_CORE_CONFORMANCE.md#issue-acceptance-ledger)
separates documentation coverage from implementation and operational evidence.
Issue closure is a maintainer/coordinator action after review, not a claim made
by this ADR. Acceptance also requires durable, publicly accessible documentation
evidence; local commits and in-app document attachments alone are insufficient.
A written proposal, passing local CI, the live static website and historical
single-host diagnostics do not qualify a distributed hosted product. This
design grants no deployment, spending, account/secret access or
environment-mutation approval.
