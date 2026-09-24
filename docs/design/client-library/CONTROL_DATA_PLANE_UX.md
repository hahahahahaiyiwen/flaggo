# Control plane and data plane UX

## Responsibility boundary

| Boundary | Owns | Credential |
| --- | --- | --- |
| Manifest tooling | Validate one authored JSON contract and generate derived artifacts | None |
| Management | Apply, review, approve, and publish exact definition snapshots | Trusted validate/apply/approve scopes |
| Runtime client | Decide under exact approved identity; confirm application | Decide/confirm scopes |
| Application Collector | Export existing native telemetry to scoped ingress | Separate ingest scope |
| Application deployment | Build/deploy code, distribute catalog and approved receipt | Developer-owned |

Using Flaggo does not replace telemetry producers or make Flaggo responsible
for application deployment. No runtime client registers a definition, approves
its own contract, or creates a telemetry provider/exporter.

## Publication

```ts
import { applyManifest } from "@flaggo/sdk/management";
const receipt = await applyManifest({ controlPlaneUrl, bundle, credential });
```

The normalized bundle is the compiler output of one JSON manifest.
Publication uses a deterministic idempotency key from authorized application,
environment, and bundle digest. Identical submissions converge on the existing
result; metadata-only changes preserve semantic definition identity.

New or changed semantics return `RequiresApprovalError` with an approval
request identity and digest. An authenticated actor approves the exact
snapshot; retrying apply obtains its stored approved receipt. Expired pending
requests are revalidated into a fresh linked approval request. There is no
automatic SDK approval or alternate previous-contract fallback.

The receipt is an approved-definition binding. The manifest currently has no
initial-authority declaration; ready-after-activation publication is follow-up
#40. Local fixtures may provision governed state, but this is not a second
public definition format or authority granted by a runtime request.

## Runtime

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const result = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});
```

Synchronous initialization captures a validated catalog/receipt pair with exact
scope, bundle digest, key set, and definition digests. Runtime sends
`definitionId + revision + contractDigest`, not the full definition or a request
to use the newest revision.

Request inputs are plain typed values. The service resolves evidence-owned
inputs from one immutable generation under authenticated tenant/application/
environment and exact definition identity. Callers cannot override them.

## Outcomes

| Outcome | Behavior |
| --- | --- |
| Approved server result | Exact compatible authority, policy, durable audit, and optional confirmation directive |
| Governed server fallback | Audited single safe default with explicit server provenance |
| SDK availability fallback | Opt-in response to recognized eligible availability failure; no server decision/audit/exposure identity |
| Invalid caller/contract/identity | Typed error; never a successful local value |
| Required input evidence missing/stale/invalid/ambiguous/future | `503 required-evidence-unavailable`, fallback-ineligible |
| Required state/audit/readiness failure | Explicit fallback-ineligible readiness/integrity error |

Retry identity is separate from correlation. Retained successful retries return
the original result even after new telemetry arrives. Failed evidence
evaluations can be retried once data becomes available.

## Confirmation and OTel

```text
application applies value -> operational confirmation -> confirmed exposure ID
  -> optional attributes on existing OTel records
```

Confirmation is never sampled telemetry. The helper supplies attributes only;
the app's providers and Collector own sampling, buffering, export, and pipeline
fan-out. The ingest credential cannot be replaced by a decide/confirm token,
and resource attributes cannot establish authorization.

Production credentials use the existing `polari.definitions:*`,
`polari.decisions:decide`, `polari.exposures:confirm`, and
`polari.telemetry:ingest` scope names. Local-development bypass is explicitly
insecure and only for trusted loopback examples.

See the [SDK reference](../../../packages/sdk-typescript/README.md) for exact
fallback classification and [OTel example](../../../examples/otel-evidence/README.md)
for complete Collector configuration.
