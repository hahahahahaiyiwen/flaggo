# TypeScript SDK

The TypeScript SDK owns the code-first `declare -> decide -> observe`
experience, bundle extraction, accepted-binding initialization, runtime calls,
exposure confirmation, and explicitly configured availability fallback.

Generated or hand-verified wire models must conform to `contracts/`. The SDK
does not hide contract/configuration errors and never treats correlation
identity as retry identity.

Update this document whenever public API, fallback, extraction, or release
behavior changes.
