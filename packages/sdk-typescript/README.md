# TypeScript SDK

`@flaggo/sdk` owns typed decision calls, exact catalog/receipt binding,
availability fallback, and explicit exposure confirmation. It does not own
telemetry instrumentation, collection, approval, strategy selection, or policy.

## Author and compile

Author one JSON manifest using
[`flaggo.decision-definition-bundle/v2`](../../contracts/schemas/decision-definition-bundle-v2.schema.json).
Each decision has one result/default, explicit targeting and policy, and
optional typed context, inputs, evidence bindings, and intent.

```powershell
flaggo-manifest build .\decision-manifest.json `
  --bundle .\generated\definitions.json --catalog .\src\generated\catalog.ts
flaggo-manifest check .\decision-manifest.json `
  --bundle .\generated\definitions.json --catalog .\src\generated\catalog.ts
```

`build` emits a normalized bundle and a literal-key TypeScript runtime catalog.
`check` fails for stale generated artifacts. Neither evaluates application
source. Inputs are plain required values owned by `request` or `evidence`;
evidence inputs inherit their binding schema and cannot be supplied by callers.

## Publish separately

Trusted management code, not a browser or runtime client, publishes the bundle:

```ts
import { applyManifest } from "@flaggo/sdk/management";

const receipt = await applyManifest({
  controlPlaneUrl,
  bundle,
  credential: managementCredential
});
```

`RequiresApprovalError` carries the pending approval identity and digest.
An authorized reviewer approves the exact snapshot; retrying apply obtains the
receipt. Neither `applyManifest` nor `createFlaggoClient` grants approval.
Publication uses a deterministic application/environment/bundle idempotency key.
Unresolved policy references fail explicitly; no policy catalog is implied.

## Runtime client

```ts
import { createFlaggoClient, confirmedExposureAttributes } from "@flaggo/sdk";
import { catalog } from "./generated/catalog.js";

const flaggo = createFlaggoClient({
  catalog,
  receipt,
  dataPlaneUrl,
  dataPlaneCredential: { mode: "bearer", getToken },
  availabilityFallback: { mode: "local-default", retries: 1 }
});

const decision = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});

gameEngine.updateConfig({ dropInterval: decision.value });
if (decision.source === "server" && decision.exposure.confirmationRequired) {
  const confirmed = await flaggo.exposures.confirm(
    decision.decisionId, decision.exposure.confirmToken
  );
  applicationLogger.emit({
    eventName: "game.outcome",
    body: "Applied drop interval.",
    attributes: confirmedExposureAttributes(confirmed)
  });
}
```

The factory is synchronous and performs no network registration or hashing.
It validates and defensively captures the catalog and receipt: exact
application/environment, bundle digest, key set, and definition digests.
The receipt supplies each opaque definition ID and revision. Calls never
select an implicit latest revision or accept inline definitions.

Generated types reject unknown/non-numeric keys, missing or wrongly typed
required caller data, and evidence-owned inputs. Runtime validation repeats
those checks before network access or fallback. Calls without required caller
fields may omit the request object. Request operands do not emit telemetry.
The runtime import graph is browser-compatible; compiler and hashing code
belong to tooling/management entry points.

`tune.number` returns a compact receipt. `tune.numberDetailed` adds policy,
target-resolution, confidence, and fallback details. Deterministic numeric
rules return `confidence: null`. Server result identity and integrity are
verified before use. `correlationId` sets `X-Flaggo-Correlation-Id`;
`idempotencyKey` separately sets `Idempotency-Key`.

## Fallback and retries

Availability fallback is disabled by default. `local-default` permits the
catalog's one result default only after configured retries for recognized
DNS/connection failures, connection/read timeouts, intermediary HTTP 502/504,
or valid explicitly eligible Flaggo 5xx problems (excluding 500/501/505).
Cancellation, TLS, authentication/configuration, malformed responses,
contract/identity errors, and ineligible problems never fall back.

Required input evidence unavailable is always fallback-ineligible. Separately
configured policy-quality evidence follows its explicit server eligibility;
it is not a source of implicit input defaults. Client fallback has no server
decision, policy, audit, or exposure identity. Governed server fallback remains
an audited server outcome.

Retries reuse one idempotency key and body; the default is one retry, with
zero through two configurable. `Retry-After` waits are capped at one second.
`409 idempotency-in-progress` permits retry within that budget, never fallback.
Successful retained replays do not re-resolve evidence.

## OpenTelemetry boundary

Use the application's OTel APIs, providers, exporters, and Collector.
`confirmedExposureAttributes` is a pure helper returning only
`flaggo.exposure.id` and `flaggo.decision.id` from an explicit confirmation.
It creates no provider, exporter, event schema, or ambient context.
Confirmation must follow application of the value and remains an operational
API call even when telemetry is sampled.

For received-telemetry inputs, declare native OTel evidence bindings in the
manifest and add a scoped exporter to the application's Collector. See the
[OTel example](../../examples/otel-evidence/README.md). There is no producer,
extractor, startup-registration, or v1 compatibility path.

## Development

```powershell
npm run build
npm run typecheck
npm test
```

Build removes this package's generated `dist` before compiling. Tests include
manifest freshness, generated typing, semantic vectors, browser isolation,
management/runtime wire behavior, and OpenAPI alignment. Shared contract
changes must also update .NET/Python conformance, fixtures, and canonical docs.
