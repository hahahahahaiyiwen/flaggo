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
fallback attribution. Adaptive decisions additionally retain full evidence and
compact confidence while the runtime response omits evidence detail.
Application and environment ownership are immutable parts of every audit
record.

Phase 3 adds a local JSON Lines adapter implementing separate decision and
confirmed-exposure audit ports. Readiness and every append validate all
existing lines with the same strict envelope/record parser and rebuild the
exposure-ID deduplication set; malformed, torn, duplicate, or wire-invalid
records make the sink unavailable. Every nonempty file must end in LF; CRLF
records are accepted because they are LF-terminated, while a valid JSON record
without its terminal newline is treated as torn and append is rejected. New
records use a platform-independent LF separator. Exposure records are appended
before the prepared confirmation is committed, so append failure cannot leave
a newly confirmed exposure without its audit record. Retry reuses the prepared
exposure ID and never appends a duplicate. Unused decision receipts therefore
create no exposure audit record. File inspection is an explicit local tool
boundary; no production runtime endpoint exposes audit contents.
