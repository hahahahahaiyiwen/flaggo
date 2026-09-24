# Client library design

## Boundary and entry points

The TypeScript client turns an approved definition into a small typed call to
Decision Service. Trusted tooling publishes to Contract Service. The application
owns applying values and its existing OpenTelemetry providers and Collector.

| Entry point | Responsibility |
| --- | --- |
| `@flaggo/sdk/manifest`, `flaggo-manifest` | Parse/validate/normalize/hash one JSON manifest; generate bundle/catalog; detect stale artifacts |
| `@flaggo/sdk/management` | Explicit trusted publication and strict receipt/approval-required validation |
| `@flaggo/sdk` | Synchronous catalog/receipt binding, typed calls, guarded results, retries/fallback, confirmation and pure correlation helper |

The runtime graph has no Node filesystem, compiler, hashing, exporter or
control-plane startup dependency. Generated artifacts are not another authoring
surface. No producer handles, inline definitions or AST extraction remain.

## Lifecycle

```text
JSON manifest -> compiler -> normalized bundle + typed runtime catalog
trusted apply -> explicit exact-snapshot approval -> approved receipt
catalog + receipt -> synchronous immutable runtime client
key + context + request-owned operands -> Decision Service
application applies result -> explicit confirmation -> optional OTel attributes
```

Publication and application deployment are independent. Browsers receive
approved bindings, never management/ingest credentials. Current receipts prove
approved definition publication, not #40's future initial-authority activation.
#49 owns executable server alignment without changing this client ownership.

## Caller contract

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const decision = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: { boardPressure, recentPlacementTimeMs, recoveryFailures, currentLevel }
});
```

Catalog literals drive key/result/context/input types. Evidence-owned inputs
are excluded from caller data. Dynamic union keys retain key/request
correlation: narrow the key or pass a discriminated tuple when requirements
differ. The same contract applies to `number` and `numberDetailed`.

JavaScript/dynamic calls repeat unknown-field, primitive/bounds and required
context checks before network access or fallback. Every declared operand is
required; the request object is optional only for eligible definitions.
Initialization checks scope, bundle digest, exact key set and definition
digests before capturing a defensive catalog/receipt copy. Opaque lineage and
revision come from the receipt, never implicit latest-revision selection.

## Wire and results

Decide sends exact identity, client metadata, context/target, plain request
inputs and separate correlation/idempotency headers, not the full definition.

`number` returns a compact receipt; `numberDetailed` adds target, constraint
evaluation, confidence, fallback and explanation metadata. Current wire fields
retain the schema's `policy` naming until #49's executable alignment.
Validate returned type/bounds/step, exact identity, source and exposure union.
Deterministic numeric rules return `confidence: null`.

Only recognized, explicitly enabled availability fallback returns the
catalog's one default with client-only provenance. Invalid caller data,
contract/integrity errors and unavailable required input evidence remain
errors. Retries retain request identity; successful replay does not drift
with new telemetry.

## Telemetry and confirmation

Manifest bindings interpret existing Gauge/span/span-event/log data received
through application-owned pipelines. The decision client defines no
instrument, producer envelope or exporter.

The application confirms only after applying a server result with a
confirmation directive. `confirmedExposureAttributes(confirmation)` supplies
ordinary exposure/decision attributes to existing OTel APIs; it does not
confirm, emit or install ambient context. Sampling cannot remove or fabricate
the operational confirmation.

## Validation and references

SDK coverage includes generated typing, invalid dynamic values, immutable
identity checks, browser isolation, manifest freshness, management outcomes,
wire fixtures, retries and explicit confirmation. Worker/Tetris prove live
inputs; the [Collector example](../../../examples/otel-evidence/README.md)
proves telemetry-owned inputs and confirmed outcomes.

- [SDK reference](../../../packages/sdk-typescript/README.md)
- [Control/data-plane UX](CONTROL_DATA_PLANE_UX.md)
- [Decision definition](../../architecture/DECISION_DEFINITION.md)
- [Shared contracts](../shared-contracts/README.md)
- [Contract Service](../contract-service/README.md)
- [Decision Service](../decision-service/README.md)
