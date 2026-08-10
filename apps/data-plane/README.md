# Data Plane

The data-plane host evaluates exact registered decision identities and confirms
exposures.

It composes registry reads, evidence, governed state, policy, reasoning, and
audit ports. Contract/configuration errors fail closed; only explicitly
eligible availability failures may reach an SDK-local fallback. Keep its wire
behavior aligned with `contracts/openapi/flaggo-runtime-v1.yaml`.

## Current implementation

`src/Flaggo.DataPlane` is the .NET 10 runtime composition root. It exposes only
decide, exposure confirmation, liveness, and readiness endpoints over
local adapters. Definition validation, application, and approval routes
are owned by `apps/control-plane`.
Runtime decisions require an
exact registered definition tuple; mismatches return contract Problem Details
rather than a fallback. Decide requests support 24-hour idempotent replay and
exposure confirmation is idempotent for the same observation.
Confirmation commits only after durable exposure audit succeeds. Audit I/O
failure returns retryable `503 service-unavailable` with correlation and
`Retry-After` metadata but remains client-fallback ineligible; exposure
confirmation commits use a five-second timeout linked to application shutdown
after audit and ignore request abort at that consistency boundary. Those
timeouts are also explicitly ineligible. Unexpected
infrastructure failure returns `500 internal-error`. Endpoint operation
metadata is captured before execution so the single global exception boundary
can preserve decide timeout eligibility while denying exposure fallback.
If a reused exposure identity conflicts with its durable audit record, the
endpoint fails closed through the existing
`409 exposure-confirmation-conflict` wire response.

Protected runtime endpoints require OAuth bearer authentication and
operation-specific
scopes. The explicit `Flaggo__Authentication__LocalDevelopmentBypass=true`
setting provides a Development-environment-only principal with runtime scopes
and the seeded application/environment claims. The host fails at startup when the
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
that cannot round-trip through the canonical IEEE-754 representation are also
rejected. Correlation identity is preserved in both headers and Problem Details
through exception handling. Decide
idempotency fingerprints include the HTTP operation identity and canonical
request body; deterministic terminal responses are retained for 24 hours, and
followers exceeding the one-second wait budget receive
`idempotency-in-progress`.

Readiness is derived from registry, state, audit, policy, and optional evidence
health ports. Required dependency loss returns `503 not-ready`; optional
evidence loss returns `200 degraded`. Unhandled infrastructure failures are
mapped to stable Problem Details. Generic I/O failures remain explicitly
client-fallback ineligible; only decide timeouts or
definition-policy-approved required-evidence failures can authorize SDK-local
fallback.

Run locally:

```powershell
dotnet run --project apps\data-plane\src\Flaggo.DataPlane
```

The seeded `tetris.dropInterval` definition supports local runtime exercises.
Registry reads reload the shared repository-local
`.flaggo/definition-registry-v1.json` under a cross-process lease, so approved
control-plane revisions become visible without restarting this host. Override
the location with an absolute `Flaggo__Registry__LocalFilePath`. Registry file,
parse, or lock failures make the required readiness dependency unavailable and
do not fall back to the seed.

Rate-limited decision failures carry matching `Retry-After` and
`retryAfterSeconds` values. Global resolution fallback retains the originating
cohort claim in target provenance while identifying the resolved global target.

Phase 3 local integration selects file-backed state and audit adapters through
`Flaggo__State__LocalFilePath`, `Flaggo__Evidence__LocalFilePath`, and
`Flaggo__Audit__LocalFilePath`. State and evidence paths name atomically
published commit descriptors; unpinned raw JSON is rejected. The state file is activated by trusted
bootstrap from an approved registration receipt; the composition root does not
hardcode the Tetris strategy. Local evidence supplies compact strategy
confidence. Exposure confirmation is composed through the constructor-injected
confirmation service and bounded post-audit commit policy, which records the
exposure audit before committing state. Its independent timeout/shutdown wait
also bounds non-cooperative state adapters; retry reconciles the durable audit
with prepared or possibly late-committed state by stable exposure identity.
Local audit inspection is file/tool based and adds no runtime debug route.

The Tetris harness instead sets `Flaggo__Bootstrap__LocalGenerationPath`.
The ASP.NET composition root registers a request-scoped
`BootstrapGenerationResolver`, `LocalFileStateStore`,
`LocalFileEvidenceProvider`, `DecisionService`, and readiness probe. The
resolver lazily reads and validates `current.json` once for the request,
validates every pinned receipt/state/evidence artifact, and gives state and
evidence the references from that same generation. A pointer switch between
the state and evidence phases therefore cannot mix generations; the next
request scope observes the newly published generation. No ambient context or
process-global generation cache is used. Registry, audit, clocks, identifiers,
strategy, policy, idempotency, and exposure services remain singleton where
their implementations are thread-safe and do not capture scoped adapters.

Direct state/evidence descriptor configuration uses the same scoped hosting
boundary and pins the configured descriptors for the request. Independent
descriptors do not form a cross-file transaction; callers that require
state/evidence atomicity use the generation manifest. Directly constructed
module adapters and in-memory ports remain available for unit/domain
composition. The receipt is published and digest-validated in a bootstrap
generation for bootstrap and SDK consumers but is not a data-plane runtime
adapter input.
