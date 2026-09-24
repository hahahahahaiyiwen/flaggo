# Audit Module

Owns immutable decision, approval, policy, exposure, and explanation records.

Audit writes are explicit async operations and failures are surfaced; callers
must not report an audited success when durable recording is required but
failed. Records preserve the exact contract identity and relevant provenance.

Update this document when retention, redaction, record shape, or durability
requirements change.

## Current implementation

`src/Flaggo.Audit` defines the async `IAuditSink` port and a thread-safe
in-memory adapter. Decision orchestration records the audit entry before
returning a server success; sink failures propagate and therefore cannot
produce a falsely audited response. Records preserve runtime context and
inputs, runtime/control targets, target provenance, resolution chain, policy
result, accepted contract identity, returned value and type, and complete
fallback attribution. `Inputs` is the resolved vector; `RequestInputs` retains
the original caller values, and `InputProvenance` records each operand's source,
binding, immutable generation, original nanosecond time, coverage and available
record/trace/span/exposure references. Selected policy-quality evidence is
separate from these input observations. Deterministic numeric rules report
null learned confidence. Authenticated tenant, application, and environment
ownership are immutable parts of every decision audit record.

Phase 3 adds a local JSON Lines adapter implementing separate decision and
confirmed-exposure audit ports. It stores bounded segments under
`<audit-path>.d/segments`. A durable `manifest.json` is the sole authority for
the ordered segment generations, current segment, byte lengths, record counts,
content hashes, and exposure record references. Each append validates only the
bounded listed current segment, durably appends, then atomically publishes the
updated manifest. Rotation creates and flushes the new uniquely named segment
before switching the manifest; no missing listed segment is ever recreated.
Readiness fully validates every listed segment and rejects missing or unlisted
segment files.

Instance-local synchronization is combined with an exclusive cross-process
lease on `<audit-path>.lock`. The lease covers bounded-segment validation,
rotation, exposure-ID lookup, append, marker recovery, and marker publication.
Markers identify the exact committed segment, record index, and record hash.
Record bytes and their manifest catalog entry are committed before marker
staging is flushed and atomically renamed. A crash in between leaves a missing
marker that the next append or readiness probe rebuilds only after resolving
and validating the cataloged exposure record. Existing corrupt, mismatched, or
orphan markers are never trusted or repaired. Fresh initialization stages and
flushes a complete empty store before publishing its directory. Incomplete
published layouts, corrupt manifests, unlisted segments, and legacy single
JSONL files fail closed; there is no automatic manifest reconstruction or
production migration policy.
Directory metadata synchronization is centralized in the shared-contracts
`DurableDirectory` helper: Unix opens and `fsync`s the directory, while
Windows opens it with backup directory semantics and calls `FlushFileBuffers`. Only documented
unsupported-operation results are treated as explicit best effort; permission,
open, and other I/O failures continue to surface.
Lock cancellation, timeout, permission, and IO failures surface to writers;
health checks report them as unavailable while still propagating cancellation.

Every nonempty file must end in LF; CRLF records are accepted because they are
LF-terminated, while a valid JSON record without its terminal newline is
treated as torn and append is rejected. New records use a
platform-independent LF separator. Exposure records are appended before the
prepared confirmation is committed, so append failure cannot leave a newly
confirmed exposure without its audit record. Retry reuses the prepared
exposure ID and never appends a duplicate. Exact exposure-record replay is a
no-op; reuse with a different decision, application, environment, applied
timestamp, or confirmed timestamp throws `ExposureAuditConflictException`.
The in-memory adapter enforces the same canonical identity under its lock, and
the confirmation endpoint maps the theoretical conflict to the frozen 409
response. Unused decision receipts therefore create no exposure audit record. File inspection is an explicit local tool
boundary; no production runtime endpoint exposes audit contents.

Before lease creation or layout staging, every missing audit parent component
is created through `DurableDirectory` and receives ordered parent and child
barriers. Concurrent creators converge; a failed barrier prevents lease or
layout publication.

Persisted decision records require an explicitly present, non-default
`recordedAt` timestamp with either `Z` or a numeric UTC offset. Confirmed and
optional applied exposure timestamps follow the same invariant. Replay also
requires defaultable scalar members such as fallback flags and confidence or
evidence quality to be present rather than accepting deserialization defaults.
Contract identities require exact `sha256:` plus 64 lowercase hexadecimal
digests (including an optional bundle digest). Digest and RFC 3339 timestamp
validation is absolute: encoded leading or trailing spaces, tabs, line feeds,
or carriage returns are rejected rather than trimmed or accepted as regex line
boundaries. Runtime-context and input
values are restricted to strings, booleans, or finite numbers that preserve
their exact value through IEEE-754 canonicalization; null, object, array,
nonfinite, and rounding-unsafe values fail readiness and append. Replay and new
writes also enforce strict input maps, frozen fallback/policy/provenance
enums, decision-mode discriminators, primitive decision values, and bounded
confidence/evidence numbers. Successful `strategy` and `experiment` records
require a strategy ID. A deterministic rule may have no policy-quality
evidence and has null confidence. When a confidence report is present, its
quality, model uncertainty, and expected outcome must match the separately
recorded evidence.
`active-value` success carries no strategy ID, evidence, or confidence.
Fallback carries no confidence; it may retain an attempted strategy ID and its
available evidence for explanation, but evidence without a strategy ID is
invalid. Evidence quality, uncertainty, and expected outcome are finite
probabilities, and sample size is finite and nonnegative.

Before any durable append, the exact serialized JSON envelope is passed through
the same strict duplicate-property scan, deserialization, shape validation,
and semantic validation used by replay. Embedded `JsonElement` values,
including evidence details, therefore cannot introduce duplicate nested
properties or any value that a restart would later reject.
Decision records also persist the returned reason so inspection can correlate
weighted inputs, strategy identity, and reasoning.
