# State Module

Owns governed decision state, cooldowns, overrides, pause/resume state,
exposure state, and rollback transition metadata.

State is keyed by immutable decision identity and control target. Semantic
contracts do not share governed state by default. Storage is accessed through
async module-owned ports so in-memory and durable adapters remain replaceable.

Update this document when lifecycle, concurrency, or persistence invariants
change.

## Current implementation

`src/Flaggo.State` defines the async `IStateStore` lookup port and an in-memory
adapter keyed by decision key, definition lineage, and runtime revision. State
also carries the canonical contract digest so orchestration can reject stale
or incompatible governed state. Governed values carry their actual control
target, optional deterministic numeric-rule strategy, and last-change time for
cooldown evaluation. Lookup follows the request resolution hierarchy so
attribution is derived from the selected state rather than independently from
client claims. The reasoning boundary supplies an ordered list already
validated against the registry-owned runtime definition projection. State
adapters must match only those exact targets and must never invent broader
fallback or reinterpret target hierarchy semantics.

The module also defines in-memory async ports for 24-hour decide idempotency
and exposure confirmation. Concurrent requests with the same key and request
fingerprint coalesce to one decision result; reusing a key for a different
request conflicts. Exposure confirmation accepts the same observation
repeatedly and conflicts if a later confirmation changes it. Confirmation is
a two-phase prepare/commit transition: preparation reserves a stable exposure
identity, and reasoning commits it only after the exposure audit succeeds.
Preparation and audit honor request cancellation. After audit is durable,
reasoning commits with a separate bounded token linked to application shutdown,
so request abort cannot interrupt the audit-first transition and commit timeout
or shutdown leaves the prepared confirmation available for an idempotent retry.
`CommitConfirmationAsync` is idempotent for the same decision/exposure identity.
This lets retry reconcile a prepared transition even when an earlier
non-cooperative call succeeds after its caller's bounded wait has ended; a
different exposure identity still conflicts.

Pending exposures retain the immutable decision-time attribution snapshot.
The snapshot includes application/environment ownership, returned treatment
value/type, fallback attribution, policy result, confidence, and full evidence;
confirmation hides records from
credentials outside that ownership scope.
Accepted confirmations and identical prepared confirmations are recognized
before first-confirmation clock validation, preserving retryability and the
original exposure identity after an audit failure. A changed observation still
conflicts while preparation is pending.
Idempotency entries retain both successful decisions and deterministic
`4xx` rejections for 24 hours. Outcome retention is classified explicitly from
the terminal status rather than exception shape: every `5xx`, including
required-evidence-unavailable `503`, releases the claim after concurrent
followers converge on that attempt. A later request with the same key can then
retry recovered dependencies. Followers exceeding the bounded wait receive an
explicit in-progress result.

Phase 3 adds a JSON-file `IStateStore` adapter for local shared-host
integration. Direct `Flaggo__State__LocalFilePath` configuration names a
strict commit descriptor, not a raw state file. The descriptor pins one
immutable sibling artifact by exact byte length and lowercase sha256.
Bootstrap composition instead supplies the shared `current.json` generation
manifest, which pins receipt, state, and evidence without redundant sidecars.
The persisted identity must come from an approved registration receipt;
missing descriptors, unpinned raw files, malformed commits, digest mismatch,
or incompatible state surface as dependency failures.

`IStateSnapshotProvider` is the module-owned local-persistence seam. A directly
constructed adapter resolves its configured commit source for each lookup.
The ASP.NET host instead injects a request-scoped provider: direct descriptors
are pinned for that request, while generation mode returns the state artifact
reference from the request's single shared state/evidence generation. The
store opens only that safe immutable sibling, reads bounded bytes, and verifies
length plus sha256 before strict JSON and typed state validation. An in-place
rewrite, including a valid parseable intermediate document paused
indefinitely, fails digest validation. Stable reads have no mandatory delay.

Trusted direct writers use `CommittedFileSnapshotWriter`: create and fsync a
new immutable artifact, fsync its parent directory, create and fsync the
descriptor temporary, atomically rename the descriptor last, and fsync the
directory again. Bootstrap/governance tooling publishes all
receipt/state/evidence artifacts in a new generation and atomically switches
its digest-pinned manifest last. Raw-file fallback is
intentionally unsupported.

The adapter accepts only the implemented `active-value` and `strategy` modes
from the frozen decision-mode enum. Active values cannot carry strategy
fields. Strategy state requires a nonempty strategy ID, a finite numeric
current value, and the supported deterministic numeric-rule contract.
`experiment`, `fallback`, unknown modes, incoherent combinations, invalid
targets, nonprimitive values, and nonfinite rule parameters make state health
unavailable.

Numeric-rule state may declare normalized weighted inputs. Every declared
input field is required and finite. Persisted decision numbers and every
numeric-rule scalar are retained as raw JSON until the shared `CanonicalJson`
IEEE-754 compatibility check succeeds, so integers or decimals that would
silently round during typed deserialization are rejected. Relevant range
checks, including ordered minimum/maximum values, nonnegative weights, and
positive finite total weight, run only after that exact compatibility check.
Omitted fields cannot silently become zero; legitimate canonical zero,
including negative zero, remains numerically valid.

The complete file is structurally validated before typed deserialization.
Malformed or trailing data and case-sensitive duplicate member names at any
nesting level make state health unavailable; the same member name remains
valid when it appears in separate sibling objects.

State identity is a structural tuple of decision key, definition id, revision,
and the optional target type/id pair. Duplicate detection and lookup use the
same ordinal tuple equality; delimiters inside any valid component, including
colons and newlines, cannot merge distinct states.

Persisted `lastChangedAt` is read as a string and parsed explicitly as RFC 3339
with `Z` or a numeric offset; offsetless and host-local interpretations are
forbidden. Accepted values are normalized to UTC before state is exposed.
Frozen v1 cooldown remains any finite nonnegative number; the local adapter
does not impose an `int.MaxValue`-seconds ceiling. Policy evaluation uses
elapsed-time comparison and remains overflow-safe for timestamps supplied by
any `IStateStore`. Digest and timestamp lexical checks are absolute and reject
encoded leading or trailing whitespace and control characters.
