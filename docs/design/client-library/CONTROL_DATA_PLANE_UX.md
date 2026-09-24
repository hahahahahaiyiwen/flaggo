# Control plane and data plane UX

## Responsibility boundary

| Boundary | Owns | Credential |
| --- | --- | --- |
| Manifest tooling | One authored JSON contract and generated bundle/catalog | None |
| Trusted Contract Service client | Apply/review/approve exact definition snapshots | Validate/apply/approve scopes |
| Runtime client / Decision Service | Exact-identity decide and application confirmation | Decide/confirm scopes |
| Application Collector / OTel Ingestion | Native scoped telemetry delivery | Separate ingest scope |
| Application deployment | Build/deploy code and distribute approved bindings | Developer-owned |

Flaggo never replaces producers or deploys application code. The runtime
client cannot register, self-approve, or create a telemetry provider/exporter.

## Publication

```ts
import { applyManifest } from "@flaggo/sdk/management";
const receipt = await applyManifest({ controlPlaneUrl, bundle, credential });
```

The bundle is compiler output from one JSON manifest. Publication uses a
deterministic idempotency key from scope and bundle digest. Identical requests
converge; metadata-only updates preserve semantic definition identity.

New/changed semantics return `RequiresApprovalError` with exact approval
identity/digest. An authenticated actor approves that snapshot, then apply
replay returns its stored receipt. Expired submissions are revalidated into a
fresh linked approval. There is no automatic SDK approval or previous-contract
fallback.

The current receipt is an approved-definition binding. #49 aligns server
contracts and stores; #40 adds initial-authority/activation-ready publication.
Trusted local fixtures can provision state but are not a public authoring
format or authority granted by a runtime call.

## Runtime

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const result = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});
```

Initialization synchronously validates scope, bundle digest, key set and
definition digests. Calls carry the exact definition ID/revision/digest.
Request operands are plain values; evidence operands come from one immutable
generation under authenticated scope and exact definition. Callers cannot
override evidence ownership.

| Outcome | Behavior |
| --- | --- |
| Approved server result | Exact authority, deterministic constraints, durable record and optional confirmation directive |
| Governed fallback | Recorded safe default with server provenance |
| SDK availability fallback | Explicit eligible outage response, without server record/exposure identity |
| Invalid caller/contract/identity | Typed error, never a successful local value |
| Unusable required input evidence | 503 `required-evidence-unavailable`, SDK-fallback-ineligible |
| Required state/record/readiness failure | Explicit integrity/readiness error |

Idempotency is distinct from correlation. Successful retry keeps the original
result/provenance despite new telemetry; a failed dependency evaluation can
retry after recovery.

## Confirmation and OTel

```text
apply value -> operational confirmation -> committed exposure ID
  -> optional attributes on existing OTel records
```

The helper supplies attributes only. Application providers and Collector own
sampling, buffering, export and fan-out. A decide/confirm token grants no ingest
permission, and resource attributes cannot establish authorization.

Current production scopes are `polari.definitions:*`,
`polari.decisions:decide`, `polari.exposures:confirm` and
`polari.telemetry:ingest`. Development bypass is explicitly insecure and
restricted to trusted local examples.

See the [SDK reference](../../../packages/sdk-typescript/README.md) for exact
fallback classification and [Collector example](../../../examples/otel-evidence/README.md)
for routing/configuration.
