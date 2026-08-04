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
target, and lookup follows the request resolution hierarchy so attribution is
derived from the state selected rather than independently from client claims.

The module also defines in-memory async ports for 24-hour decide idempotency
and exposure confirmation. Concurrent requests with the same key and request
fingerprint coalesce to one decision result; reusing a key for a different
request conflicts. Exposure confirmation accepts the same observation
repeatedly and conflicts if a later confirmation changes it.

Pending exposures retain the immutable decision-time attribution snapshot.
The snapshot includes application/environment ownership, returned treatment
value/type, and fallback attribution; confirmation hides records from
credentials outside that ownership scope.
Accepted exposure confirmations are replayed before first-confirmation clock
validation, preserving the original exposure identity across later retries.
Idempotency entries retain both successful decisions and deterministic
rejections; infrastructure failures release the claim, and followers exceeding
the bounded wait receive an explicit in-progress result.
