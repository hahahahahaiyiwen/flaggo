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
