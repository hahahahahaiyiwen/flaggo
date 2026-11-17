# Portable infrastructure behavior decisions

## Status

Supporting design for [the portable-core ADR](PORTABLE_CORE.md), issue #93.
D2-D5 models and the D7 threat-model baseline have dated approvals below or in
the ADR. Proposed protocol details and unfilled operating policies are not
approved implementations. Operation names below are semantic descriptions, not
committed Rust traits or public endpoints. Existing wire semantics remain
authoritative until explicitly revised.

The narrowed #93 gate approves architecture/behavioral models and their
qualification plan, not operational policy values. Workload/SLO and
retention/deletion follow-ups are tracked in
[hahahahahaiyiwen/flaggo-cloud#5](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/5)
and [hahahahahaiyiwen/flaggo-cloud#6](https://github.com/hahahahahaiyiwen/flaggo-cloud/issues/6),
respectively, with public summaries in the
[ADR's hosted operating envelope](PORTABLE_CORE.md#d6-d7-required-hosted-operating-envelope).
Their unresolved values continue to block dependent hosted gates even after
the documentation gate is accepted. Concrete wire schemas and adapter protocols
remain implementation work under #94 and the relevant hosted phases.

## D2: authorization and scope

**Resource-API model approved 2026-11-17:** the maintainer selected "Approve
this authorization and error model (Recommended)" after reviewing independent
authentication, explicit scope matching, permission-filtered inventory and the
401/404/403 distinctions below.

**Ingestion model approved 2026-11-17:** the maintainer selected "Approve
whole-batch authorization before durable acknowledgement (Recommended)" after
reviewing all-or-nothing scope checks, explicitly authorized multi-scope batches
and retention of the local unauthenticated adapter.

`AuthorityScope` is declared resource identity, never a credential.
Authentication resolves an independently verified principal and allowed
operations/scopes. Identity-provider integration is private; the public core
defines the authorization context and required checks. A local trusted policy
is explicit composition, not evidence that hosted authentication exists.

For existing resources: authenticate, resolve the explicit public identity
through trusted code, authorize its stored authority and requested operation,
then expose content or perform work. Never use claims to choose a different
resource. For creates/validation: authorize the requested explicit scope and
verify body/route agreement before name reservation or detailed diagnostics.
Resource identifiers/digests are not bearer capabilities.

| Boundary | Required authorized scope and protection |
| --- | --- |
| Contract management and decisions | Full resource authority and operation; exact name/digest binding; no implicit current lookup in decisions |
| Catalog/history/list/health | Filter before paging/counting; cursors and ETags bound to authorized scope; global worker catalog requires a distinct service grant; no detailed store diagnostics exposed through public health |
| Candidate admission/activation | Service principal constrained to contract/cycle and operation; analysis can propose, never self-activate |
| OTLP ingestion | Trusted authenticated envelope records allowed scope and producer context; all declared Resource scopes must be permitted before durable acknowledgement |
| Materialization | Recheck declared scope against the persisted authorized envelope; preserve scope in identities, deduplication and diagnostics; do not trust raw `flaggo.*` attributes |
| Evidence query | Trusted scope derived from authorized contract and its evidence sources; the query cannot broaden scope or read physical tables |
| Workspace/artifact/execution | Opaque handles bound to scope, cycle, attempt and allowed action; authorize each read/write/commit, including history, recovery and signed-download issuance |

Approved HTTP distinctions: missing/invalid authentication is 401;
authenticated access to an absent or inaccessible resource has the same 404
shape; forbidden operations on an otherwise visible resource are 403.
Name reservations, errors, pagination and timing must not disclose another
scope's inventory. Internal diagnostics may distinguish denial/not-found but
must be access controlled and redact payloads and credentials.
These responses and security requirements need versioned wire/profile coverage;
they are not current v3 guarantees.

An OTLP request can contain several Resources. Permit a multi-scope batch only
when the credential explicitly permits every declared scope. Reject a batch
with missing/forbidden scope before acknowledgement, without partial writes.
Keep source routing, relevance, identity extraction and semantic diagnostics in
the materializer. This adds an ingestion **authorization check**, intentionally
different from today's receiver, which only validates transport and appends
bytes without extracting authority. Do not call it unchanged OTLP behavior.

## D3: authority transitions and activation visibility

**Consistency model approved 2026-11-17:** the maintainer selected "Approve
strong authority consistency (Recommended)" after reviewing post-acknowledgement
visibility, captured in-flight executables, atomic publication/admission/
activation, immutable-only caching and explicit failure during authority
partitions. Operation signatures and any changed wire retry semantics still
require executable-contract review.

### Atomic outcomes

Use semantic commands that encompass all related records, even if the
implementation has separate logical Contract Store and Executable Store owners.
Checks performed in an API process before a later write are insufficient.

| Operation | Preconditions and indivisible durable outcome | Retry/race outcome |
| --- | --- | --- |
| Publish accepted-ready contract | Authorized resource identity; validated immutable contract/default/optional compilable authored artifact; reserve ownership, persist version/artifacts, establish bootstrap activation if absent and publish management current coherently | Identical version preserves acceptance identity and never resets an already active learned executable; concurrent versions have a serializable publication order |
| Admit analysis candidate | Exact accepted/current contract and authorized cycle; atomically verify currentness and operation identity, insert immutable artifact and admission provenance | Same identity/content returns recorded admission; different content under that identity conflicts without an orphan artifact |
| Activate candidate | Expected candidate state/version, expected active token, still-current contract and applicable learning policy; atomically replace active state and resolve/supersede admissions | At most one competing transition wins; stale tokens conflict/no-op as specified; failed validation/write preserves prior activation |
| Read runtime authority | Exact authorized name/digest, accepted-ready contract and one committed active identity; verify immutable artifact integrity | Complete old or new activation for overlapping requests, never torn or missing authority caused by replacement |

Default generation and authored compilation remain shared product behavior.
Internal staging is permitted, but cannot be exposed as accepted-ready or leave
a successfully published version without an active executable. An interrupted
staging operation must be recoverable/idempotent. Do not require a distributed
transaction with object storage: stage immutable bytes first, then conditionally
publish verified durable references; unreferenced uploads are reclaimable only
under the approved retention policy.

Proposed acceptance ordering: absent a versioned expected-current precondition,
concurrent successful PUTs serialize by publication order; the last publication
selects management current. An old-version redeploy may select that version
again, as today; a delayed retry is not silently treated as a protected
"do not change current" command. Stronger deployment intent/idempotency would
require an explicit contract change. Runtime still uses exact digests.

Current deployment uses separate store calls, not one accepted-ready transaction.
Current admission checks currentness under a process-local mutex, then writes
the artifact/admission transaction. Current activation already checks the
contract-current row and lifecycle tokens inside a SQLite transaction. Extract
that good prior art without assuming that two independent provider calls can
replace one atomic operation.

### Activation read guarantee

Approved guarantee: linearizable active-state selection per contract digest.
A runtime lookup that begins after an activation acknowledgement observes that
activation or a later committed one. A lookup concurrent with activation may
select either complete state; once selected, its immutable executable remains
valid for that in-flight evaluation. No global ordering across unrelated
contracts is required.

Mutable activation cannot use a TTL cache or an unconstrained read replica.
Cache immutable contract/executable/compiled artifacts by verified identity;
read active state authoritatively or use a protocol that proves the same
consistency. Authority unavailability fails explicitly even when an old compiled
artifact is cached. An eventual-consistency mode would be a separate approved,
visible guarantee, not an adapter optimization.

Use state-version tokens, not executable digest alone, for mutation fences:
an A -> B -> A transition must invalidate a token from the first A. A token
must not wrap or be reused in its resource lifetime. Internal lifecycle/fencing
tokens are not new inputs to stateless runtime decisions.

The same input and same executable yield the same result. A transport retry
after activation can select a different executable; stateless decisions do not
promise response replay or pin authority across retries.

## D4: acknowledgement, retry and worker ownership

**Guarantee model approved 2026-11-17:** the maintainer selected "Approve
fenced at-least-once execution and durable reconciliation (Recommended)" after
reviewing monotonic ownership epochs, stale-write rejection, atomic projection/
checkpoint commits, uncertain-outcome reconciliation and OTLP observation-level
deduplication without an exactly-once or payload-hash promise. Concrete
protocols, durations and retention windows remain to be specified.

### Identities and durable acknowledgements

| Operation | Durable retry identity | Meaning and limitations |
| --- | --- | --- |
| Contract version creation | Explicit resource identity plus contract digest | Immutable insertion is idempotent; current-pointer redeploy ordering is described above |
| Candidate admission | Scoped workspace and cycle plus matching submission payload, including original attempt and content/manifest identity | Recovery reuses sealed provenance; a new attempt is not permission to submit a second candidate; conflicting retry fails |
| Activation | Candidate/active state tokens and selected candidate | Duplicate completed work reconciles from durable state; an old expected token cannot activate again |
| Raw OTLP append | Durable inbox batch identity; no current sender idempotency key | Success follows durable complete-request append; lost response may cause another batch on retry |
| Evidence projection | Partition/checkpoint position plus stable observation identity/content | Retry cannot duplicate logical effects; identical observation is deduplicated and differing content records a conflict |
| Artifact/handoff publication | Scoped cycle/attempt, immutable artifact digest and expected state revision | Repeated publication returns committed outcome or conflicts; uploading bytes alone does not advance workflow |
| Execution attempt | Scoped cycle plus execution-attempt identity | Recover by querying attempt status/outputs; starting a new attempt is not replaying an arbitrary side effect exactly once |

Do not deduplicate whole OTLP requests solely by payload hash: separate legitimate
exports may have identical bytes. Preserve existing observation identity rules
and version any changed rule. No end-to-end exactly-once claim is made. The
minimum recommendation retains standard OTLP retries with observation-level
deduplication; any optional producer idempotency key/window needs a separately
documented extension. Retention of dedup records cannot silently be shorter
than the advertised retry/recovery window. The separate retention/deletion
follow-up owns those unresolved durations; the invariant is part of D4.

On timeout, connection loss or cancellation near commit, the caller may not
know whether the operation committed. Return an explicit uncertain/dependency
outcome and reconcile using durable identity/state; never report rollback just
because the caller stopped waiting. If commit is known to have succeeded,
return/recover that receipt. Conflicting identities are not transient failures.
Candidate reconciliation is read-only and still authorized; it does not
reactivate superseded work. Existing v3 submissions check currentness before
duplicate lookup; changing that response ordering requires explicit coverage.
The current admission key is workspace/cycle, but equality also checks the
original attempt, contract name/digest, executable digest and manifest digest.
Do not silently weaken those checks while extracting a portable retry port.

### Materialization and fencing

Current `EvidenceStore::commit` already atomically writes observations,
deduplication/conflict/provenance records, diagnostics and the forward checkpoint.
It assumes one active materializer per database. Version changes are provenance,
not permission to replay completed batches.

Recommendation: one ordered logical partition initially, with an exclusive
durable claim and monotonically increasing epoch. This requires neither a
broker nor sharding. Each commit supplies the claim epoch and expected previous
checkpoint; storage validates both atomically with the entire projection.
If partitions are later introduced, document the ordering/dedup scope first.
An allocated batch sequence alone is insufficient if concurrent appends can
commit out of order: a consumer must not advance past an uncommitted earlier
batch. Use a committed ordered stream or an explicitly proven gap protocol.

Every mutable worker operation validates the current fence at the authoritative
commit boundary: checkpoint, cycle/handoff, budget, outcome and candidate
publication. Heartbeats or checking a lease just before an unguarded write do
not fence a paused worker. Candidate admission must verify the authorized cycle
and ownership epoch atomically with admission, not trust a submitted integer.
If claims and mutations use different systems, a design must prove how that
check is atomic; otherwise co-locate the relevant authority.

Storage decides lease validity using its authoritative time/order, not a worker
wall clock. Lease duration/renewal/backoff are hosted operating inputs, not
fixed by #93. After lease loss: stop starting work, request execution
cancellation and reject stale commits even if the old process continues
running. A new owner gets a strictly newer epoch, reconciles committed records, and resumes only
unfinished work. Cancellation is best effort for computation but mandatory
for preventing subsequent unauthorized state changes.

| Failure injection | Required observable outcome |
| --- | --- |
| Crash before inbox commit | No acknowledgement and no committed receipt |
| Crash after inbox commit, before response | Durable batch survives; retry may append another batch; projection deduplicates according to observation identity |
| Crash during projection | All effects plus checkpoint, or none; no checkpoint-only success |
| Lease expires while worker is paused | Replacement can claim; old worker cannot advance any fenced state after resuming |
| New contract supersedes analysis | Reject new stale admission/activation; retain already committed provenance and last valid active executable |
| Cancellation races commit | Reconcile authoritative outcome; do not blindly repeat non-idempotent effects |
| Retention overtakes consumer | Explicit gap/loss diagnostics; never silently fabricate materialized success |
| Authority unavailable | Fail or pause explicitly; never fall back to process-local authority |

Inbox acknowledgement means retained durable ingestion, not eventual
materialization forever. Existing receipt-age retention is independent of
consumer progress. Hosted retention/backlog policy and supported loss envelope
must be approved through the separate follow-ups before dependent hosted
acceptance; local default limits do not establish hosted guarantees.

## Durable artifacts versus execution (D4/D7 proposal)

The workspace control capability owns cycle/attempt identities, claims,
budgets, handoff pointers, candidate receipts and recovery. The artifact
capability owns scoped immutable bytes, hashes and conditional manifest
publication. Execution receives only authorized input handles, an execution
profile and resource/network limits, and returns a durable attempt receipt
plus bounded output/artifact references.

Successful state publication must reference durable, integrity-checked inputs;
a failed upload cannot become a committed handoff. Recovery follows the last
committed handoff pointer, not the newest object/listing. Atomic rename,
directory enumeration, host `PathBuf` and object-store list consistency are
not substitutes for conditional publication. Local filesystem views may adapt
these capabilities for tools; they are not the portable storage protocol.

Keep local Python explicitly trusted and unsandboxed. Hosted analysis must not
inherit the service environment or general store credentials. Bound CPU,
memory, wall time, process count, output, disk and egress with the approved
isolation mechanism; separate execution capacity from workflow claims.
The D7 baseline assumes malicious tenants and untrusted generated Python;
trusted operators/control plane are explicitly outside that initial attacker
model. Concrete controls and limits remain pending. "Run in a container"
alone is not isolation acceptance.

## D5: evidence-query portability

**Portability model approved 2026-11-17:** the maintainer selected "Capability
negotiation with shared result/scope semantics and required scenarios
(Recommended)" after reviewing the dialect/subset alternatives. This approves
negotiation, authorized relations, lossless result types, bounded execution and
truthful provenance, not an arbitrary-SQL compatibility promise or a final
result schema.

Current `EvidenceAnalysisStore` advertises SQLite `SELECT`/non-recursive `WITH`,
an `observations` relation and allowlisted functions. It queries live committed
evidence for each call: full authority, permitted sources, and
`contract_digest IS NULL OR contract_digest = requested`. Subsequent queries may
include late evidence. Saved query results are immutable analysis artifacts;
there is no cycle-wide frozen database watermark.

Current result columns expose SQLite declared types, with unknown expression
types. `observation_ids` contains only IDs explicitly projected in returned
rows; an aggregate does not produce complete input lineage. Neither these
types nor arbitrary SQLite JSON/arithmetic semantics are a portable contract.

Approved direction: negotiate an explicit versioned dialect/capability profile.
Keep local SQLite supported; require every provider to implement the same
authorized logical relations, bounded execution, result envelope and a shared
set of representative analysis scenarios. An agent must inspect capabilities
and fail unsupported operations, not retry invented syntax indefinitely.
Do not promise that arbitrary SQL text or model-generated analysis is portable.

Alternatives: a deliberately restricted common SQL subset gives stronger
text-level parity but requires a specified grammar, numeric/JSON semantics and
translation/conformance maintenance; forcing every provider to emulate SQLite
adds execution/storage coupling. Neither is silently required by a second adapter.

The proposed portable envelope records dialect/profile version, normalized
query, authorized scope/source selection, limits, execution/read metadata,
ordered column names, canonical value kinds and nullability, rows, truncation,
result hash and explicitly qualified provenance. Unknown/variant expression
types must remain representable even for empty results; no row-based guessing.

Specify lossless integer representation (including values above JavaScript's
safe range), finite float representation, exact double bits where applicable,
text, boolean, JSON, null and unknown/variant types in a versioned result schema
before implementation. Preserve existing evidence distinctions between int64,
double, numeric-looking string and non-finite double bits. Reject unsupported
values rather than lossy conversion; do not relabel provider type strings as
canonical types.

Authorize before querying, and make the scoped relation the only data surface.
Prohibit physical tables, writes, extensions, external IO and unapproved
functions. Enforce row/byte/deadline limits at execution, including backend
cancellation and connection reuse. Distinguish empty complete results,
row-truncated results, byte/time limit errors and dependency failures.
Deterministic comparisons require explicit ordering; provider-specific
collation, NULL, division, overflow and aggregate behavior must be advertised
and fixture-tested, not assumed equal.

Record query identity, scope, profile, read context and saved-result digest for
all queries. Label returned observation IDs as projected IDs, not complete
aggregate lineage. Recommend reproducibility from persisted result artifacts,
not an unbounded replay promise against changing data. A true snapshot or full
aggregate lineage capability is separately negotiated and retention-bound.
Concrete type schema/profile revision and its minimum scenario set require
executable-contract review and artifacts under #94 before any portability claim.
