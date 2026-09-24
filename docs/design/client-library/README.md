# Client library design

## Boundary and goals

The TypeScript client turns an approved decision contract into a small typed
runtime call. The application remains responsible for applying values and for
its existing OpenTelemetry instrumentation, providers, and Collector.

Keep three entry points separate:

| Entry point | Responsibility |
| --- | --- |
| `@flaggo/sdk/manifest`, `flaggo-manifest` | Parse one JSON manifest, validate/normalize/hash, generate bundle and typed catalog, detect stale artifacts |
| `@flaggo/sdk/management` | Explicit trusted manifest apply and strict receipt/approval-required validation |
| `@flaggo/sdk` | Synchronous catalog/receipt binding, typed calls, guarded results, retries/fallback, confirmation, pure confirmed-attribute helper |

The runtime import graph has no Node filesystem, compiler, hashing, exporter,
or control-plane startup dependency. Generated files are derived artifacts,
not a second contract authoring surface.

## Lifecycle

```text
one JSON manifest -> compiler -> publication bundle + runtime catalog
trusted management apply -> explicit exact-snapshot approval -> receipt
catalog + receipt -> synchronous immutable runtime client
key + context + request-owned operands -> decide
application applies value -> explicit confirmation -> optional OTel attributes
```

Publication is independent of application deployment. Browsers receive approved
bindings, not management or ingestion credentials. A definition receipt proves
approved publication, not the initial-authority activation extension owned by
[#40](https://github.com/hahahahahaiyiwen/flaggo/issues/40).

## Caller contract

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const decision = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});
```

Catalog literals drive key/result/context/request-input types. Evidence inputs
are excluded from caller inputs. JavaScript/dynamic calls receive equivalent
runtime checks, including unknown inputs, primitive type, finite bounds, and
required context. No request is sent and no fallback is returned for invalid
caller data. Every declared operand is required; no defaults are invented.

Initialization checks application/environment, bundle digest, exact key set,
and every definition digest against the approved receipt, then captures a
validated defensive copy. Opaque IDs/revisions come from the receipt.
Per-call definitions, producer handles, and implicit latest-revision lookup
are not supported.

## Wire and results

Requests contain exact expected identity, client metadata, context/target,
plain request-input maps, and separate correlation/idempotency headers.
The full contract and evidence bindings never travel with decide.

`number` returns a compact receipt; `numberDetailed` adds target, policy,
confidence, fallback, and explanation metadata. Validate the returned type,
bounds/step, expected identity, source, and exposure directive.
Deterministic rules may return `confidence: null`; do not manufacture learned
confidence from observed telemetry.

Recognized, explicitly configured availability fallback returns the single
catalog default with client-only provenance. Contract/readiness errors and
unavailable required input evidence remain errors. Retries preserve request
identity; retained success does not drift as telemetry arrives.

## Telemetry and exposure

The SDK does not define application metrics/events or own export transport.
Manifest bindings interpret already instrumented Gauge, span/span-event, and
log data received through an application-owned Collector.

The application confirms only after applying a server value with a confirmation
directive. `confirmedExposureAttributes(confirmation)` supplies two ordinary
attributes for the application's configured OTel APIs. It does not confirm,
emit, create exporters, or install ambient state. Telemetry sampling cannot
remove or fabricate the operational confirmation.

## Validation

The SDK suite exercises literal catalog typing, invalid dynamic values,
immutable binding/identity checks, browser import isolation, manifest
compilation/freshness, management outcomes, wire fixtures, retries, and explicit
confirmation. Worker/Tetris examples cover live operands; the
[stock Collector example](../../../examples/otel-evidence/README.md) covers
native telemetry-owned operands and confirmed outcomes.

## References

- [SDK API and commands](../../../packages/sdk-typescript/README.md)
- [Control/data-plane UX](CONTROL_DATA_PLANE_UX.md)
- [Decision definition](../../architecture/DECISION_DEFINITION.md)
- [Shared contracts](../shared-contracts/README.md)
