# TypeScript SDK

`@flaggo/sdk` owns the TypeScript application boundary for Flaggo decisions:
canonical bundle registration, accepted runtime bindings, numeric decision
calls, exposure confirmation, typed signals, and telemetry emission. It does
not implement policy, strategy selection, approval, or server state.

## Client lifecycle

```ts
import { createFlaggoClient } from "@flaggo/sdk";

const flaggo = await createFlaggoClient({
  appId: "tetris-demo",
  environment: "dev",
  dataPlaneUrl: "http://localhost:8080",
  controlPlane: {
    mode: "startup-register",
    url: "http://localhost:8081",
    bundle,
    credential: { mode: "local-development" },
  },
});

const decision = await flaggo.tune.number("tetris.dropInterval", {
  definition: bundle.definitions[0],
  context: { sessionId: "game-456" },
  correlationId: "game-loop-123",
  idempotencyKey: "tick-456",
});
```

Startup registration must finish before a client is returned. A semantic
change requiring approval rejects initialization with `RequiresApprovalError`.
Production callers can use `{ mode: "bearer", getToken }` credentials.
`local-development` is an explicit insecure bypass for trusted local use.
Already registered workloads may instead pass a registration receipt with
`controlPlane.mode: "pre-registered"`.

Registration receipts and `requires-approval` responses are validated as
strict wire contracts before their identity or metadata is trusted. An
approved receipt must contain exactly one accepted binding for every submitted
definition, with matching canonical contract digests.

Every runtime call recomputes the local definition digest and compares it with
`receipt.acceptedDefinitions[decisionKey]` before network access or fallback.
The data-plane request contains only compact accepted identity and runtime
values; full definitions are never sent.

## Results and fallback

`tune.number` returns a compact `DecisionReceipt<number>`.
`tune.numberDetailed` returns provenance, policy, confidence, fallback, and
target-resolution details. Server results are accepted only when the returned
definition ID, revision, digest, bundle/build/deployment identity, and
integrity match the expected binding.

Availability fallback is disabled by default. Enabling
`availabilityFallback: { mode: "local-default" }` permits the declared default
only after retries are exhausted for DNS, refused/reset connection, or
connection/read timeout failures; intermediary HTTP 502/504 responses; or a
valid Flaggo 5xx Problem Details response with
`clientFallback.eligible: true`, except HTTP 500, 501, and 505.
Cancellation, TLS/certificate, proxy/authentication/configuration, malformed
response, contract, and identity errors never fall back. A client fallback has
no server decision, policy, audit, or exposure identity.

The default is one retry after the initial attempt. Configure zero, one, or two
with `availabilityFallback.retries`. Retries reuse a caller-supplied
`Idempotency-Key`, or one generated for that decision call, and honor
`Retry-After` for at most one second.

`409 idempotency-in-progress` is retry-only: the SDK waits within the same
retry budget and resends the identical body and key, then surfaces the 409 if
the budget is exhausted. It never authorizes local fallback.

Correlation and retry identity remain separate:

- `correlationId` becomes only `X-Flaggo-Correlation-Id`.
- `idempotencyKey` becomes only `Idempotency-Key`.

## Signals and telemetry

`createSignalHandle` creates event or app-emitted metric producers whose value
types are inferred from the declaration. `createInferenceSignalHandle` accepts
only app-emitted primitive metrics and creates runtime inputs of that metric's
declared type. `createDerivedMetricHandle` creates a non-emitting
derived-metric identity whose `valueType` cannot disagree with its declaration.
All handles verify a supplied `schemaDigest`.

Definition authoring types expose boolean, number, and string action-space
unions. Reference policies require `policyId`; inline policies require a
typed constraint array and optionally declare governed client fallback.

Telemetry is emitted through the `TelemetrySink` interface. Use
`createOpenTelemetrySink(logger)` with a structurally compatible OpenTelemetry
logger, or provide a direct sink for local development and tests.

## Contract maintenance

Wire models are hand-verified against `contracts/`. Any wire-semantic change
must update OpenAPI, JSON Schema, fixtures, and conformance before this package.
Keep canonical normalization aligned with
`contracts/conformance/validate.py`, add fixture-driven tests for behavior
changes, and update this README whenever public API, fallback, extraction, or
release behavior changes.
