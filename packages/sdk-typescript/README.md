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
Digest and RFC 3339 validators require exact whole-string matches; encoded
trailing line breaks or other boundary characters are not accepted.

At startup, the client normalizes the supplied bundle and caches each numeric
definition and contract digest. Runtime calls use that static binding and
compare it with `receipt.acceptedDefinitions[decisionKey]` before network
access or fallback. When a static bundle is configured, a call's `definition`
property remains authoring input for extraction but is not hashed or consumed
at runtime; the registered bundle is authoritative. Callers using a
pre-registered receipt without its bundle may pass `definition` explicitly;
that compatibility path validates the definition on each call.

## Static extraction

`flaggo-extract` scans TypeScript application sources before compilation and
emits `flaggo.static-extraction/v1`, containing a canonical definition bundle,
its digest, and source-located decision descriptors. Configure startup
registration with the generated `bundle`:

```powershell
flaggo-extract --project .\tsconfig.json --app tetris-demo `
  --environment dev --out .\.flaggo\definitions.json
```

The MVP extractor accepts direct `flaggo.tune.number("literal-key", { ... })`
calls whose `definition` is an object literal composed only of JSON literals,
literal arrays, and literal objects. Directly imported signal-handle constants
are allowed where the frozen contract expects a signal reference; extraction
resolves them to `{ key }` and includes their literal declarations. Supported
factories are `createSignalHandle`, `createInferenceSignalHandle`, and
`createDerivedMetricHandle`. Parentheses, `as const`, and `satisfies` wrappers
are allowed.

Extraction fails the build for dynamic decision keys, spreads, computed or
shorthand properties, method declarations, function calls inside static
definitions, non-literal definition references, conditional or loop-dependent
decision calls, and schema-invalid definitions. Identical definitions for one
key deduplicate in the bundle; different canonical digests for one key fail
with `contract-conflict`.
Authored cooldown constraints may use any finite nonnegative number, preserving
the frozen v1 contract. Static extraction rejects negative and nonfinite values
before emitting an artifact.
Runtime expressions remain in `context`, `runtimeTarget`, and `inputs`; they
are never evaluated by extraction or included in definition identity.

The data-plane request contains only compact accepted identity and runtime
values; full definitions are never sent. Its client identity reads
`sdkVersion` from this package's metadata so release version bumps cannot
drift from runtime telemetry.

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
For `required-evidence-unavailable`, both the definition's effective
client-fallback policy and this SDK availability configuration must permit the
local default; either side forbidding fallback surfaces `FlaggoHttpError`.
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

The Phase 3 Tetris contract declares `tetris.outcomeObserved`. After applying a
server decision and confirming its exposure, the frontend emits this event
through a configured `TelemetrySink` with the confirmed `decisionId` and
`exposureId`. This preserves explicit attribution without adding an
incompatible runtime endpoint. Client-fallback results and unused receipts
have no exposure identity and must not emit a linked outcome.

## Contract maintenance

`npm run check:openapi` parses both frozen OpenAPI YAML documents and verifies
the startup apply, decide, and exposure operations, OAuth scopes, headers, and
the external request/response schemas required by the SDK. `npm test` also runs
this gate. Any wire-semantic change must update OpenAPI, JSON Schema, fixtures,
and conformance before this package.
Keep canonical normalization aligned with
`contracts/conformance/validate.py`, add fixture-driven tests for behavior
changes, and update this README whenever public API, fallback, extraction, or
release behavior changes.
