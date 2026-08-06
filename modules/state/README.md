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
client claims.

The module also defines in-memory async ports for 24-hour decide idempotency
and exposure confirmation. Concurrent requests with the same key and request
fingerprint coalesce to one decision result; reusing a key for a different
request conflicts. Exposure confirmation accepts the same observation
repeatedly and conflicts if a later confirmation changes it. Confirmation is
a two-phase prepare/commit transition: preparation reserves a stable exposure
identity, and reasoning commits it only after the exposure audit succeeds.

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
integration. It reloads immutable activated state for each lookup so trusted
bootstrap or local governance tooling can replace the file atomically without
restarting the data plane. The persisted identity must come from an approved
registration receipt; malformed, missing, or incompatible files surface as
dependency failures rather than silently selecting another state.

Each lookup holds a snapshot read handle that permits readers and
delete/rename sharing but denies write sharing. An in-place truncate or rewrite
therefore cannot race the copy and produce a torn state document. Trusted
writers must write and flush a complete sibling file and atomically replace the
configured path; the current lookup finishes against the old file identity and
the next lookup observes the replacement.

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
The adapter uses the shared runtime policy ceiling of `int.MaxValue` seconds and
rejects timestamps later than `DateTimeOffset.MaxValue` minus that duration,
as an adapter-level defense. Policy evaluation independently uses elapsed-time
comparison and remains overflow-safe for timestamps supplied by any
`IStateStore`. Digest and timestamp lexical checks are absolute and reject
encoded leading or trailing whitespace and control characters.
