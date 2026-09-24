# Data Plane

The data-plane host evaluates exact registered decision identities and confirms
exposures, and receives scoped binary OTLP/HTTP telemetry.

It composes registry reads, evidence, governed state, policy, reasoning, and
audit ports. Contract/configuration errors fail closed; only explicitly
eligible availability failures may reach an SDK-local fallback. Keep its wire
behavior aligned with `contracts/openapi/flaggo-runtime-v1.yaml`.

Decision execution receives `IRuntimeDefinitionReader`; native ingress receives
the separate registry-owned `IEvidenceBindingReader`. Neither parses
registry persistence or authors definitions.

## Current implementation

`src/Flaggo.DataPlane` is the .NET 10 runtime composition root. It exposes
decide, exposure confirmation, native OTLP, liveness, and readiness endpoints over
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
and the configured application/environment claims. The resource scope defaults
to `tetris-demo`/`dev`; local scenarios can override it through
`Flaggo__Authentication__LocalDevelopmentAppId` and
`Flaggo__Authentication__LocalDevelopmentEnvironment`. The host fails at startup when the
bypass is enabled in another environment or when neither the bypass nor OAuth
authority and audience are configured. OAuth credentials must carry
`polari_tenant_id`, `polari_app_id`, and `polari_environment` claims. Resource
claims must match the request body or OTLP route.
Credentials with multiple resource claims retain the validated request scope
for idempotency and may confirm exposures owned by any authorized scope.
Idempotency namespaces use the authenticated `polari_tenant_id` together with
the validated application and environment, so credential rotation does not
change replay identity.

Request parsing rejects unsupported media types, duplicate JSON properties,
incorrectly cased or unknown members, null input entries, legacy input arrays, and malformed digest
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
Target lookup follows the registered inference target and explicit fallback
order, accepts request targets only from the registered hierarchy, and rejects
missing required or inconsistent target-bearing runtime context before state
lookup.

Phase 3 local integration selects file-backed state and audit adapters through
`Flaggo__State__LocalFilePath`, `Flaggo__Evidence__LocalFilePath`, and
`Flaggo__Audit__LocalFilePath`. State and evidence paths name atomically
published commit descriptors; unpinned raw JSON is rejected. The state file is activated by trusted
bootstrap from an approved registration receipt; the composition root does not
hardcode the Tetris strategy. Policy-quality evidence is separate from native
input materialization; request-only rules with no evidence policy need neither
provider and return null learned confidence. Exposure confirmation is composed through the constructor-injected
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

Local scenarios may add authoritative cohort aliases under
`Flaggo__Targeting__AuthoritativeCohorts__<claimed>=<resolved>`. These mappings
augment the existing Tetris development aliases and are passed to the
constructor-injected target resolver; empty identifiers fail host startup.

## Native telemetry

`POST /otlp/{appId}/{environment}/v1/{metrics,traces,logs}` accepts
`application/x-protobuf` with identity/no compression or gzip. Success uses the
corresponding native export response; failures use `google.rpc.Status`, not
Problem Details. Ingest requires `polari.telemetry:ingest` and the authorized
tenant/app/environment. Development-only ingest uses
`Authorization: Flaggo-Local-Telemetry`; ordinary runtime credentials do not
grant ingest permission.

The adapter normalizes Gauge points, spans and span events, and structured
logs. It preserves typed metric stream identity (including resource/scope and
point attributes, independent of attribute order), exact nanosecond times,
and available trace/span flags. Evidence interprets the accepted manifest
bindings and commits one immutable generation before acknowledgement.
Matched invalid observations produce native partial-success diagnostics.
Unmatched telemetry is counted and discarded. Shutdown drains materialization
and releases its writer lease before completing host stop.

`Flaggo__Telemetry__*` settings:

| Setting | Default |
| --- | --- |
| `CommitDescriptorPath` | `data/telemetry/inputs.commit.json`, relative to host content root |
| `MaximumBodyBytes` | 4 MiB, separately enforced on encoded and decompressed bytes |
| `MaximumRecords` | 1,000, including span-event work |
| `MaximumBindings` | 1,000 |
| `MaximumFrames` | 10,000 |
| `MaximumSnapshotBytes` | 16 MiB |

All limits are positive integers. Oversized transport/work returns 413;
snapshot capacity returns 429; unavailable durable publication returns 503.
The latter two carry `Retry-After: 1`. Persistence corruption is never replaced
with an empty success. Missing/stale/future/ambiguous/unusable required inputs
fail decide with `required-evidence-unavailable` and local fallback forbidden.
See [the Collector example](../../examples/otel-evidence/README.md).
