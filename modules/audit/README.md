# Audit Module

Owns immutable decision, approval, policy, exposure, and explanation records.

Audit writes are explicit async operations and failures are surfaced; callers
must not report an audited success when durable recording is required but
failed. Records preserve the exact contract identity and relevant provenance.

Update this document when retention, redaction, record shape, or durability
requirements change.
