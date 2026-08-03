# Data Plane

The data-plane host evaluates exact registered decision identities and confirms
exposures.

It composes registry reads, evidence, governed state, policy, reasoning, and
audit ports. Contract/configuration errors fail closed; only explicitly
eligible availability failures may reach an SDK-local fallback. Keep its wire
behavior aligned with `contracts/openapi/flaggo-runtime-v1.yaml`.

## Current implementation

`src/Flaggo.DataPlane` is the .NET 8 composition root. The first vertical slice
exposes decide, exposure confirmation, liveness, and readiness endpoints over
in-memory registry, state, and audit adapters. Runtime decisions require an
exact registered definition tuple; mismatches return contract Problem Details
rather than a fallback. Decide requests support 24-hour idempotent replay and
exposure confirmation is idempotent for the same observation.

Runtime endpoints require OAuth bearer authentication and operation-specific
scopes. The explicit `Flaggo__Authentication__LocalDevelopmentBypass=true`
setting provides a Development-environment-only principal with both scopes and
the seeded application/environment claims. The host fails at startup when the
bypass is enabled in another environment or when neither the bypass nor OAuth
authority and audience are configured. OAuth credentials must carry
`polari_app_id` and `polari_environment` claims matching the request body.
Credentials with multiple resource claims retain the validated request scope
for idempotency and may confirm exposures owned by any authorized scope.
Idempotency namespaces use the authenticated `polari_tenant_id` together with
the validated application and environment, so credential rotation does not
change replay identity.

Request parsing rejects unsupported media types, duplicate JSON properties,
incorrectly cased or unknown members, null input entries, and malformed digest
identities before evaluation. Explicit null optional members and numeric values
outside the canonical IEEE-754 range are also rejected. Correlation identity is preserved in both
headers and Problem Details through exception handling. Decide
idempotency fingerprints include the HTTP operation identity and canonical
request body; deterministic terminal responses are retained for 24 hours, and
followers exceeding the one-second wait budget receive
`idempotency-in-progress`.

Readiness is derived from registry, state, audit, policy, and optional evidence
checks. Required dependency loss returns `503 not-ready`; optional evidence
loss returns `200 degraded`. Unhandled infrastructure failures are mapped to
stable Problem Details.

Run locally:

```powershell
dotnet run --project apps\data-plane\src\Flaggo.DataPlane
```

The seeded `tetris.dropInterval` definition exists only to exercise the frozen
Phase 1 contract while persistent control-plane registration is implemented.
