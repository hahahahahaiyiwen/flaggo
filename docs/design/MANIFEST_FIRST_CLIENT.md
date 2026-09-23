# Manifest-first decisions and OpenTelemetry evidence

Status: accepted implementation design; implementation pending.

Delivery issue: [#44](https://github.com/hahahahahaiyiwen/flaggo/issues/44).
Ordering: the accepted
[redesign-first plan](https://github.com/hahahahahaiyiwen/flaggo/issues/44#issuecomment-5799702891).
Inspected implementation baseline: `f9ab77b7f0966577f5d0365293c0a153100ff717`.

This document specifies the replacement to implement in #44, not behavior
already available. Executable schemas and tests remain authoritative for the
current implementation. Approval does not start implementation.

The user explicitly confirmed that backward compatibility with the current
implementation and bundle shape is not required. Select the target contract for
clarity; replace obsolete code and fixtures rather than constrain the design
around them.

## Goal and constraints

Using Flaggo must not require replacing an application's telemetry producers.
The application authors one static decision manifest, publishes it through the
control plane, and calls the decision client with a key and genuinely live data.
Existing OpenTelemetry instrumentation reaches Flaggo through an
application-owned Collector pipeline.

The manifest owns how a decision interprets selected observations. It does not
own their production, collection, exporters, credentials, buffering, or sampling.
An input contract is not a telemetry instrument: supplying `boardPressure` to a
decision never emits a measurement.

Preserve exact accepted definition identity, authenticated approval, bounded
execution, explicit policy/fallback, durable decision audit, and explicit
exposure confirmation. Do not turn delayed telemetry into current application
state merely to make the call parameterless.

## Decisions

| Choice | Decision and reason |
| --- | --- |
| Authoring | One hand-authored JSON manifest is the only source. Retain the bundle as the distribution concept, not its current field layout. |
| Shape | A map of decisions, one result contract/default, explicit context/targeting, typed inputs, and evidence bindings. No repeated decision key, result type, or fallback value. |
| Versioning | Publish bundle format `flaggo.decision-definition-bundle/v2`; reject v1. Replace runtime/management contracts together, without parallel formats, route aliases, or old-payload adapters. |
| Client | Initialize from a generated runtime catalog and an exact approved receipt. Move manifest publication to a separate management entry point. |
| Inputs | Definition-local, typed names with explicit `request` or `evidence` ownership. Every declared runtime input is required. |
| Evidence | Decision-local bindings select native OTel data. No global Flaggo signal declarations or producer schema digests. |
| First projection | Latest scalar observation from a Gauge, selected span/span event, or structured log. No rolling aggregation, rate DSL, counter conversion, histogram estimation, or arbitrary query engine. |
| Collection | OTLP/HTTP binary Protobuf, including uncompressed and gzip requests, in the existing data-plane host. The Collector remains application-owned. |
| Materialization | Ingestion commits bounded, immutable input snapshots outside decision evaluation. The hot path reads one snapshot generation, never raw telemetry. |
| Sampling | This slice accepts explicitly observed-only evidence. It cannot certify complete populations or correct sampling bias. |
| Correlation | A small pure helper supplies attributes from an explicit confirmation result; it creates no provider, exporter, event envelope, or ambient exposure state. |

Neither existing DTOs nor existing operation URLs impose a compatibility
constraint. Keep an existing operation name only where it remains the clearest
boundary; remove obsolete surfaces instead of maintaining alternatives.
SDK, service, executable artifacts, fixtures, and examples change together.
The bundle format version is not a Phase 3 milestone number: #40 consumes this
manifest and versions any later breaking authority change deliberately.

## Current seams and the necessary changes

| Inspected surface | Current behavior | #44 change |
| --- | --- | --- |
| `packages/sdk-typescript/src/client.ts` | Can already consume a static bundle, but also accepts per-call definitions and can register during client creation. | Require a compiled catalog/receipt binding; separate management from the runtime client. |
| `src/signals.ts`, `src/extractor.ts`, `src/static-schema.ts` | Producer handles, schema digests, and code extraction define input identity. The OTel sink wraps values in `flaggo.signal` logs. | Remove these producer/extraction paths and replace them with manifest compilation and plain input values. |
| `contracts/schemas/decision-definition-bundle-v1.schema.json` | Inference inputs and objectives refer to declared Flaggo signals. | Replace those references with typed input declarations and OTel evidence bindings. |
| `modules/registry/.../DefinitionLifecycle.cs` | Resolves input types by looking up bundle signal declarations. | Validate and project the manifest's input/source and evidence contracts directly. |
| `modules/evidence/.../EvidenceProvider.cs` | Looks up model-style quality snapshots by strategy ID; it is not a native OTel materializer. | Add a distinct input-evidence read/materialization boundary, without equating telemetry coverage with model confidence. |
| `modules/reasoning/.../DecisionPorts.cs` | Numeric execution takes governed state plus an evidence snapshot and manufactures its confidence report from that snapshot. | Pass only the runtime definition, numeric rule, and resolved typed inputs. Numeric execution does not create learned confidence. |
| `DecisionService.cs`, runtime schemas, SDK result guards | Reject a strategy candidate with null confidence. | Permit the input-only deterministic result with explicit `confidence: null`. |
| `GovernedDecisionState.cs`, audit records | Preserve the input vector for decisions and confirmations. | Preserve the resolved vector and its source provenance instead of only caller-supplied signal inputs. |

The current Tetris fixture explicitly declares an evidence-quality policy, but
that fixture is not a requirement to preserve a model-evidence dependency.
Replace the example with the accepted live-input behavior and test a
request-input-only rule without either evidence reader. Any independently
supported explicit policy guard retains a dedicated behavior test; do not
preserve old example policy choices, confidence fixtures, or failure cases
merely to keep the current implementation unchanged.

## Ownership and flow

```text
hand-authored manifest
  -> validate/canonicalize/compile
  -> trusted management apply + approval
  -> approved receipt
  -> generated catalog + receipt initialize the decision client

application OTel APIs/providers
  -> existing Collector pipelines
  -> OTLP ingress adapter
  -> scoped binding projection + durable snapshot publication
  -> input-evidence reader

key + live context + request-owned inputs
  -> exact definition lookup + scope/context validation
  -> authorized target resolution
  -> resolve request/evidence inputs from one snapshot generation
  -> existing authority selection + bounded execution + policy
  -> durable decision audit + receipt
  -> application applies value
  -> explicit exposure confirmation
  -> existing telemetry may attach the confirmed exposure attributes
```

| Boundary | Owner |
| --- | --- |
| JSON manifest, wire DTOs, canonical fixtures | Shared contracts and `contracts/` |
| Manifest validation, exact identities, runtime and binding projections | Registry |
| Manifest compiler and generated TypeScript catalog | SDK tooling entry point |
| Decide, result validation, availability fallback, confirmation | Runtime SDK |
| Manifest apply/approval interaction | Separate SDK management entry point |
| OTel wire decoding, HTTP/authentication/limits | Host adapter, not the SDK or evidence domain |
| Observation selection, latest-value reduction, snapshot persistence/quality | Evidence |
| Typed runtime input resolution and execution orchestration | Reasoning/Decision API |
| Confirmed-exposure lookup | State-owned read port |
| Resolved values/provenance and confirmation audit | Audit and existing exposure flow |

No additional deployable service is required. Do not introduce a generic
transformation framework, telemetry warehouse, or background proposal loop.

## Manifest contract

### Clean definition shape

The manifest has `format`, `application: { id, environment }`, and a `decisions`
map. The map key is the stable decision key, not a field repeated inside each
entry. Each entry owns:

| Field | Meaning |
| --- | --- |
| `result` | One discriminated result type with its bounds/allowed values and singular safe `default` |
| `context` | Typed caller context fields, requiredness, and explicit target-ID bindings |
| `targeting` | Permitted hierarchy, primary target, and ordered fallback targets |
| `inputs` | Required typed operands and explicit request/evidence ownership |
| `evidence` | Decision-local interpretations of existing OTel sources |
| `intent` | Natural-language intent or numeric objectives over evidence bindings |
| `policy` | Explicit canonical safety constraints or a governed policy reference |

The numeric result contract is `{ type: "number", min, max, step?, default }`.
Boolean/string results use the corresponding typed default and, for strings,
optional allowed values. There is no separate `valueType`, `actionSpace.type`,
or `fallback.value` in the authored definition. The one default supplies the
governed default and, only when explicitly enabled, the SDK availability
fallback; their provenance remains different. The fixed-default policy baseline
is this value, not a previous result.

Optional owner/source/build metadata is not execution semantics. Registry
lineage IDs, accepted revisions, and digests are receipt/catalog outputs, not
values an application must hand-author. Management resolves the stable lineage
under the authorized application/environment/key, captures the expected
baseline for approval, and retains conflict/replay checks; this is not runtime
"latest revision" selection.

`result`, `targeting`, and `policy` are required. Context, inputs, and evidence
may be omitted when empty; absence normalizes to an empty map, not to invented
values. An omitted context-field `required` normalizes to false. Target-bearing
context is string-typed, each target type has one context source, the hierarchy
is unique, and primary/fallback targets must belong to it. The result default
must satisfy its type, bounds, allowed values, and numeric step.

`inputs` is an object keyed by definition-local input name:

```ts
type InputMeaning = {
  source: "request";
  meaning: string;
};

type RequestInput = InputMeaning & (
  | { type: "number"; unit?: string; range?: [number, number] }
  | { type: "boolean" }
  | { type: "string" }
);

type EvidenceInput = {
  source: "evidence";
  binding: string;
};

type DecisionInputs = Record<string, RequestInput | EvidenceInput>;
```

Every listed input is required. There are no implicit values, nullable operands,
caller overrides of evidence values, or optional-input defaulting rules.
`unit` and `range` are numeric-only; bounds are ordered, inclusive, and finite.
Runtime values must
match the declared primitive type; numbers must be finite. Request input units
are a contract with the caller, not an implicit conversion instruction.

An evidence-owned input inherits type, unit, and bounds from its referenced
binding. Do not duplicate that schema beside the reference. Names are local to
the decision: `boardPressure` need not equal an OTel instrument name.

Remove `onlineStrategy` and its redundant `liveInputs` list entirely. Do not
replace them with a temporary legacy/external-authority mode. #44 resolves
already governed state through the state boundary; a decision definition does
not grant an application permission to create authority.

#40 owns the initial-authority/lifecycle extension of this same manifest and its
activation/readiness behavior. Its absence in #44 is not implicit approval or
an advertised, unimplemented configuration. Existing trusted local state
provisioning can supply acceptance fixtures, but does not dictate the new public
contract or become a second definition-authoring source.

### Example: a live-input decision with reusable evidence

This example illustrates the #44 manifest shape. Its four Tetris operands remain
live caller data; the span binding is not substituted for the current placement
time. It does not claim that #40's bundle activation is implemented.

```json
{
  "format": "flaggo.decision-definition-bundle/v2",
  "application": { "id": "tetris-demo", "environment": "dev" },
  "decisions": {
    "tetris.dropInterval": {
      "result": {
        "type": "number", "min": 200, "max": 1500,
        "step": 50, "default": 800
      },
      "context": {
        "sessionId": { "type": "string", "target": "session", "required": true },
        "cohort": { "type": "string", "target": "cohort" }
      },
      "targeting": {
        "hierarchy": ["session", "cohort", "global"],
        "primary": "session",
        "fallbackOrder": ["cohort", "global"]
      },
      "inputs": {
        "boardPressure": {
          "source": "request", "type": "number", "unit": "1",
          "range": [0, 1], "meaning": "Current occupied-board fraction."
        },
        "recentPlacementTimeMs": {
          "source": "request", "type": "number", "unit": "ms",
          "range": [0, 2000], "meaning": "Application's current placement-time estimate."
        },
        "recoveryFailures": {
          "source": "request", "type": "number", "range": [0, 5],
          "meaning": "Current recovery-failure count supplied by the game."
        },
        "currentLevel": {
          "source": "request", "type": "number", "range": [0, 20],
          "meaning": "Current game level."
        }
      },
      "evidence": {
        "latestPlacementMs": {
          "meaning": "Duration of the latest received completed placement span for this session.",
          "type": "number", "unit": "ms", "range": [0, 2000],
          "source": {
            "kind": "span",
            "name": "game.place",
            "scope": { "name": "game.instrumentation" },
            "resourceAttributes": { "service.name": "tetris-game" },
            "value": { "from": "duration" }
          },
          "projection": { "kind": "latest" },
          "target": {
            "type": "session",
            "idAttribute": { "from": "attributes", "key": "session.id" }
          },
          "freshness": { "maxAgeSeconds": 30 },
          "sampling": { "accept": "observed" },
          "attribution": { "kind": "none" }
        }
      },
      "intent": {
        "type": "natural-language",
        "text": "Keep gameplay challenging but playable."
      },
      "policy": {
        "kind": "inline",
        "constraints": [{ "kind": "max-delta", "value": 50 }]
      }
    }
  }
}
```

A different definition can explicitly select an asynchronously materialized
value as an operand:

```json
{
  "recentPlacementTimeMs": {
    "source": "evidence",
    "binding": "latestPlacementMs"
  }
}
```

That is an alternative `inputs` declaration, not a runtime option.
It changes semantic identity and requires the normal approval. A session binding
still needs the session context. Key-only calls are possible for definitions
with no required caller data, including appropriately declared global evidence
inputs; evidence ownership does not magically supply target identifiers.

### Evidence binding and selector rules

Each `evidence` entry has the fields shown above: `meaning`, result `type`,
optional result `unit`/numeric `range`, `source`, `projection`, `target`,
`freshness`, `sampling`, and `attribution`. All except result unit/range are
required. Unknown contract fields and unsupported combinations are errors.

| Source | Required selector/value shape | Supported result |
| --- | --- | --- |
| Metric | `kind: "metric"`, exact `name`, `dataType: "gauge"`, scope, resource filters, `value: { from: "value" }` | Finite numeric Gauge data-point value; exact declared metric unit |
| Span | `kind: "span"`, exact `name`, scope, resource filters; `value.from` is `duration` or `attributes` with `key` | Duration in explicitly declared `ms`, or a primitive span attribute |
| Span event | Span selector plus exact `eventName`; `value: { from: "eventAttributes", key: "..." }` | Primitive attribute of a matching timestamped span event |
| Log | `kind: "log"`, scope, resource filters, exact `eventName` or scalar `bodyEquals`; value from `attributes`/`key` or `body`/`path` | Primitive attribute or structured body field |

`scope` requires its exact instrumentation-scope name; optional version is an
exact match. Resource, record `attributes`, and optional `eventAttributes`
filters are conjunctions of exact primitive equality. No regex, coercion,
wildcard metric name, JSONPath, SQL, arbitrary expression, or cross-trace join is
accepted. A log body path is a literal list of object keys, not executable
syntax; it does not parse JSON embedded inside a string.

Resource, record, and span-event attributes are separate namespaces. An
`idAttribute` uses `from: "resourceAttributes"`, `"attributes"`, or
`"eventAttributes"` plus `key`; the latter is legal only for span events.
Non-global IDs must be nonempty strings. A global target has only
`{ "type": "global" }`. Its scope is still the authorized application/environment.

The target type must be in the definition's hierarchy. A non-global binding
used by a required runtime input must have the corresponding required
target-bearing context field in this first slice. Runtime reads use the
validated target-resolution plan, never an arbitrary target ID supplied as an
input override. Choosing a broader control-state fallback does not implicitly
broaden the evidence binding's target.

Gauge units must match OTel `Metric.unit` exactly. Span duration is a checked
nanosecond difference converted to `ms`; a negative duration is invalid.
Logs and attributes have no intrinsic unit metadata: the binding documents
their meaning/unit, and any producer unit marker can be matched explicitly by
a selector. Do not pretend to verify a log field's physical unit from its
numeric value alone.

Missing selected values, type/unit mismatch, non-finite values, no-recorded-value
metric flags, and invalid target/timestamp data cannot become zero. A matched,
identifiable newer invalid observation invalidates that target's latest value
rather than silently retaining a last-good value.

Sums, histograms, summaries, aggregate counters, population rates, rolling
windows, and arbitrary reductions are unsupported in #44. A producer's
already-computed numeric Gauge can be selected, but Flaggo must not relabel a
span/log latest value as an average, rate, or population measurement.
The old derived-signal expression strings are removed, not parsed by a new DSL.

### Objectives and semantic identity

`intent.type: "numeric-objective"` expresses numeric optimization intent; its
primary/secondary entries reference `evidence: "<binding-key>"` instead of
`signal`. The referenced result must be numeric; `minimize`, `maximize`, and
finite `target` retain their current validation. All evidence-role references
resolve to defined bindings; there is no generated `signals.allowed` list.

Existing declaration-only derived-metric fixtures must become explicit
supported OTel-binding fixtures or unsupported-projection negative cases.
Do not claim their old rate-expression strings had a working materializer, and
do not silently reinterpret an actual application objective as a latest sample.
The current live-input Tetris integration has no rate-materialization outcome
that must be implemented here.

Reuse the current canonical JSON and SHA-256 mechanisms in TypeScript, .NET,
and Python. Hash each normalized decision as `{ key, contract }`, where `key`
comes from the `decisions` map and `contract` contains that entry's execution
semantics. Include input names/sources/types/meaning, complete binding
semantics, selectors, target mapping, freshness, attribution, and sampling
requirements in the definition digest. Changing them creates a new contract.
Changing live input values does not.

The manifest's format, application identity, and normalized contents determine
the bundle digest; optional bundle metadata can change that publication digest
without changing a decision's semantic digest. Registry-issued identity and
nonsemantic owner/build/source metadata are excluded from the decision digest.
Map property order is irrelevant; ordered target
fallbacks and body-path segments remain ordered. Duplicate JSON properties are
invalid. Do not hash one catalog projection as if it were the full definition.

## Manifest compilation and the client

Replace `flaggo-extract` with `flaggo-manifest build` and a corresponding
freshness/check mode for CI. The compiler accepts JSON, validates it, uses the
existing canonicalizer, and emits:

1. The normalized bundle for trusted publication.
2. A generated TypeScript runtime catalog with literal decision keys, result
   types, required context/request-input schemas, bounds/defaults, and the
   full definition/bundle digests computed from the authored bundle.

These are derived artifacts, not additional authoring formats. A stale-artifact
check regenerates and compares them. Reuse the existing TypeScript compiler API
for safe generated syntax and `json-canonicalize` for canonicalization; keep
compiler/filesystem/hash code out of the runtime import graph. Reuse the
existing finite-number, bounds/step, strict-JSON, and policy validation helpers
where applicable, not the obsolete authoring shapes. JSON-only authoring avoids
another parser/normalization surface in the first release.

The core factory becomes synchronous and takes an immutable catalog plus an
approved receipt, not control-plane credentials or per-call definitions:

```ts
import { createFlaggoClient } from "@flaggo/sdk";
import { catalog } from "./flaggo.generated.js";

const flaggo = createFlaggoClient({
  catalog,
  receipt,
  dataPlaneUrl,
  dataPlaneCredential,
  availabilityFallback: { mode: "local-default" }
});

const decision = await flaggo.tune.number("tetris.dropInterval", {
  context: { sessionId },
  inputs: {
    boardPressure,
    recentPlacementTimeMs,
    recoveryFailures,
    currentLevel
  }
});
```

Preserve `number`, `numberDetailed`, confirmation, the minimal/full result
distinction, and the existing narrowly classified availability-fallback rules.
Do not add new result primitives as incidental scope: `number` accepts only
numeric keys. Generated typing rejects unknown keys and missing/wrongly typed
required context or request inputs. Runtime validation repeats those checks,
including unknown/evidence-owned input keys, for JavaScript and dynamic data.

The request argument may be omitted only when the catalog proves there are no
required caller fields. There is no mutable global session, implicit request
input default, or ambient exposure. Reject missing data before making a request
or applying a local fallback.

At initialization, verify application/environment, bundle digest, exact key
set, and every accepted definition digest against the catalog. Capture a
defensive validated copy. The receipt supplies `definitionId` and `revision`;
the key never selects an implicit latest revision. Use browser-compatible
runtime APIs; Node/compiler-only imports belong in tooling/management.

The runtime wire keeps exact `expectedContract`, client scope, target/context,
idempotency, and correlation fields. Replace the signal-input array with:

```json
{
  "inputs": {
    "boardPressure": 0.82,
    "recentPlacementTimeMs": 1420,
    "recoveryFailures": 2,
    "currentLevel": 3
  }
}
```

Only request-owned inputs cross this field. Omit it or send an empty object
when none are caller-owned. Old arrays, `SignalRef`, `schemaDigest`, and
per-call static `definition` payloads are rejected rather than translated.

Expose explicit publication through `@flaggo/sdk/management`, extracting the
existing apply/receipt verification behavior. A `requires-approval` result stays
an actionable control-plane outcome; neither helper nor runtime client approves
it automatically. The core client never registers through decide.

#44 returns an approved-definition receipt only after exact contract approval
and publication; do not label it an initial-authority readiness receipt.
It must not fabricate activation IDs or claim #40's stronger ready-after-
activation outcome. #40 adds that behavior to the manifest and receipt it
consumes. Receipt DTOs may change with the new manifest; no old shape is required.

## OTLP ingress and Collector routing

Add scoped binary OTLP/HTTP routes to the existing data-plane host:

```text
/otlp/{appId}/{environment}/v1/metrics
/otlp/{appId}/{environment}/v1/traces
/otlp/{appId}/{environment}/v1/logs
```

The `v1` here is the standard OTel signal protocol, not the Flaggo manifest
version. JSON OTLP, gRPC, and profiles are not advertised as supported in this
slice. Reject unsupported content types/encodings explicitly.

Generate protocol models from a pinned stable `opentelemetry-proto` release,
with only the required common/resource/metric/trace/log/collector files and
upstream licensing/provenance. Upstream explicitly supports copying and building
the proto definitions. Use maintained `Google.Protobuf`, build-only
`Grpc.Tools`, and `Google.Api.CommonProtos` for `google.rpc.Status`; do not
write a Protobuf decoder or depend on an SDK exporter as a receiver library.
Keep generated wire types inside the host adapter, outside domain interfaces.
No packages are added by this design-only change.

Follow OTLP response semantics, not Flaggo JSON Problem Details:

- Accept no compression and gzip, with a decompressed-body size bound.
- Return the appropriate `Export*ServiceResponse` with HTTP 200 only after
  accepted observations and resulting snapshots have committed.
- Return accurate partial-success rejection counts and bounded diagnostics
  for rejected observations. A partly rejected request is not a retry signal.
- Malformed data is HTTP 400 with Protobuf `google.rpc.Status`; oversized input
  is 413. Authentication/scope failures are 401/403.
- Backpressure or unavailable durable storage is 429/503 with an appropriate
  retry hint. Never acknowledge a volatile queue as durable acceptance.
- Unknown optional OTLP fields follow Protobuf forward-compatibility rules;
  unsupported Flaggo binding semantics fail manifest validation.

Unmatched telemetry is outside the configured evidence projection and is
discarded with bounded diagnostic counters, not retained as a warehouse.
Matched-but-invalid observations are reported explicitly. For one record
matching multiple bindings, count OTLP rejection once only when no matching
projection accepted it; report individual binding failures separately.

Add a separate ingest permission following the current auth convention
(`polari.telemetry:ingest`). Decide/confirm credentials do not grant ingestion.
Use authenticated tenant/application/environment scope throughout ingestion and
runtime reads; pass a verified scope object from hosting rather than accepting
tenant identity in telemetry attributes or client JSON. Route app/environment
values must be authorized by the principal. Resource attributes remain metadata.

Transport limits and the snapshot-store location are host settings. Bound
decoded request size, records per batch, and active binding/target entries;
validate finite limits at startup. Use the existing durable file publication
and no-follow helpers. Size/capacity failures must be explicit, not silent
eviction of required fresh input.

An application adds an exporter to its existing Collector. For a local
composition, the additional exporter/pipeline configuration is conceptually:

```yaml
exporters:
  otlp_http/flaggo:
    endpoint: ${env:FLAGGO_OTLP_BASE_URL}
    encoding: proto
    compression: gzip
    headers:
      Authorization: ${env:FLAGGO_OTLP_AUTHORIZATION}
    retry_on_failure:
      enabled: true
    sending_queue:
      enabled: true

service:
  pipelines:
    metrics/flaggo:
      receivers: [otlp]
      exporters: [otlp_http/flaggo]
    traces/flaggo:
      receivers: [otlp]
      exporters: [otlp_http/flaggo]
    logs/flaggo:
      receivers: [otlp]
      exporters: [otlp_http/flaggo]
```

This is an addition to an existing receiver configuration, not a complete
standalone Collector file. For example the base URL ends with
`/otlp/tetris-demo/dev`; the exporter appends each `/v1/<signal>` suffix.
Preserve existing backend pipelines and deliberately place filtering/sampling
on the desired branches. The runnable example must pin a compatible Collector
distribution and validate a complete configuration using `otlp_http`, not its
deprecated alias. Bind local receivers to loopback; do not ship credentials
in the manifest, generated catalog, or browser.

## Materialization, freshness, and storage

Registry owns a read port for the accepted, non-retired binding projections of
an authorized scope. A projection contains exact definition identity and its
validated binding semantics. Ingress pins that projection; it does not execute
unapproved manifests or reinterpret an observation under a later revision.

Evidence owns an ingest port accepting normalized, scoped observations and a
batch read port returning immutable materialized values. The host adapter
decodes native OTel; the evidence domain applies binding semantics. The internal
observation contract is not a new public producer envelope.

The first reducer is `latest`, not a processing-time rolling window:

1. Use Gauge point time, span end time, span-event time, or log timestamp as the
   observation time. Preserve original nanosecond timestamps without lossy
   JavaScript integer conversion.
2. Missing/invalid timestamps are not replaced by ingestion time. Future
   observations cannot be read as fresh, and clock regression cannot make a
   negative age valid.
3. Exact redelivery is idempotent. Older observations cannot overwrite newer
   ones or refresh their age. Conflicting latest values at the same timestamp
   produce an explicit ambiguity rather than arrival-order selection.
4. Gauge stream identity includes resource, scope, metric name/type/unit, and
   point attributes. Multiple simultaneously fresh streams for one binding
   target are ambiguous; do not average them or use last-writer-wins. Narrow
   the selector/target if a single-series value is intended.
5. Span/log selection means the latest matching received record for that
   target, not the latest event that occurred anywhere in the application.

Store frames under authenticated scope plus exact definition identity, binding
key, and target. A frame contains the scalar or an explicit unusable status,
source observation time, materialization time/generation, source record or
stream fingerprint, available trace/span IDs, observed-only coverage metadata,
and any verified exposure reference.

`maxAgeSeconds` is a positive integer duration representable by the runtime
clock arithmetic. Reject overflow at validation. A value is fresh only when
`0 <= now - observedAt <= maxAgeSeconds`. Freshness uses source time, not receipt,
replay, file modification, or query time. New definition revisions start with
their own binding frames; no implicit latest-revision or cross-revision reuse.

Use a bounded local file store built on `CommittedFileSnapshotWriter` and its
verified readers. Hold one writer lease for the local store and serialize
concurrent ingestion updates. Publish an immutable generation durably before
swapping the in-memory read view or acknowledging accepted data. Restart loads
the committed generation; corruption or uncertain publication fails closed
until the committed state is verified, not to an empty/default snapshot.

A decision captures one immutable view and one evaluation time, then looks up
only its declared evidence inputs. It does not scan raw records, aggregate,
wait for exports, or perform one independently changing read per input.
Expired frames can be reclaimed on ingestion; decision reads do not mutate
history. Capacity handling must preserve the declared resource bound.

Processing an OTLP export may materialize and commit directly before replying.
It is asynchronous relative to application decision calls; no additional
worker, outbox, or raw-event queue is needed for this slice.

## Runtime resolution, errors, and audit

Introduce a reasoning-owned input resolver consuming the registered input
contracts, verified request scope, target-resolution plan, request values, and
the evidence module's batch reader. Do not let the SDK select a binding,
snapshot generation, or evidence-owned operand.

Resolution precedes rule execution and produces a typed input vector with
per-input provenance. Unknown/missing/wrongly typed request inputs are contract
errors, not policy fallbacks. Evidence unavailability is an explicit operational
outcome, not an observation of zero or a fabricated successful decision.

| Situation | Required outcome |
| --- | --- |
| Invalid manifest binding/type/reference/operator | Management validation failure with a stable issue code and JSON Pointer |
| Duplicate JSON property, old input-array shape | Existing strict wire/schema rejection; no last-key-wins interpretation |
| Missing, unknown, wrong-type, or out-of-range caller input | HTTP 422 `invalid-inference-input`, with input-specific issue/path |
| Caller supplies an evidence-owned input | HTTP 422 source-conflict issue; reject rather than override/ignore |
| Missing, stale, ambiguous, future, invalid, or unavailable required evidence | HTTP 503 `required-evidence-unavailable`, explicit reason/binding, `clientFallback.eligible: false` |
| No required evidence inputs and no evidence-dependent policy | Do not access either evidence provider; Collector outage is irrelevant |
| Same retained decide idempotency key and same request | Return the original result, resolved values, and provenance |
| Changed request under the same key | Existing idempotency conflict; do not re-resolve under a new fingerprint |

The existing HTTP idempotency boundary already wraps evaluation. Keep the
fingerprint based on caller input and exact identity, not current evidence
generation/time. Successful retries cannot drift as telemetry arrives.
Preserve the current retry/non-retention behavior for unsuccessful evaluations.

The numeric executor receives only the runtime definition projection, numeric
rule, and resolved typed values. Active-value selection remains orchestration,
not an excuse to pass the entire lifecycle state into the numeric executor.
Rename producer-coupled input DTO/reference members to input terminology
throughout their schemas, fixtures, registry, state-rule references, and tests;
do not introduce `SignalInput` compatibility aliases.

Numeric execution returns `confidence: null`. Remove the service's
missing-confidence rejection and update every result/schema/SDK guard that
currently requires a report. Do not synthesize confidence from a fresh Gauge,
an observation count, or a source's sampled flag.

Existing explicitly selected policy-quality evidence is a different domain
contract from input materialization. It stays outside the executor; query it
only when the effective policy actually requires it. Do not turn OTel input
coverage into `ModelUncertainty`, `ExpectedOutcome`, or a made-up quality score.
Remove obsolete fixture-only confidence requirements and their example
failure paths instead of retaining them as transitional behavior.

Audit the final resolved input vector, input ownership, binding/exact definition
identity, observation time, snapshot generation, quality/coverage state, and
source/exposure references needed to reconstruct the result. Keep the original
caller vector distinguishable from resolved evidence-owned values. Avoid
copying unrelated raw telemetry or resource attributes into audit.

Exposure snapshots copy this resolved vector and provenance at decision time;
confirmation accepts no replacement inputs. Preserve the minimal public
receipt and existing authorized inspection seams rather than adding a raw
telemetry query API.

## Exposure attribution and sampling

`confirmedExposureAttributes(confirmation)` is a pure SDK utility returning
`flaggo.exposure.id` and `flaggo.decision.id`. The application attaches those
attributes to its existing OTel span/log only after it has applied the value and
awaited explicit confirmation. It may instrument Flaggo calls using its own
configured OTel APIs. No Flaggo provider/exporter or mutable global baggage is
created, and baggage is not claimed to appear automatically on recorded signals.

`attribution.kind: "confirmed-exposure"` requires an explicit exposure-ID
attribute selector and is supported for logs/spans/span events, not Gauge
series. The state-owned confirmed-exposure reader verifies scope, completed
confirmation, exact definition identity, and target linkage before an
observation can become attributed evidence. A pending decision, raw
`decisionId`, or matching trace/session ID is insufficient. Foreign, unknown,
or conflicting exposure references are rejected, never silently reclassified.

The reader exposes only completed confirmation facts; an exposure audit append
by itself is not proof that the post-audit confirmation commit completed.
Add a narrow confirmed-ID lookup to the existing state implementation rather
than changing #39's activation lifecycle.

The current exposure store is in-memory. #44 must not claim that new late
observations can resolve lost confirmations after a process restart. Previously
validated, durably materialized frames retain their recorded provenance;
new unresolvable attributions fail closed. Durable confirmation recovery is an
independent follow-up, not a new prerequisite or a fabricated audit-based
confirmation in this issue.

Do not put unique exposure IDs on every metric series. A metric exemplar is
not complete exposure coverage and cannot attribute its whole aggregate to one
exposure.

Sampling rules are intentionally conservative:

- SDK head sampling can drop spans before export. Collector fan-out cannot
  recover them.
- Tail sampling happens downstream on received spans and may be biased toward
  errors or slow operations. It does not establish a complete trace population.
- Logs can be filtered or sampled at the logging system, SDK, Collector, or
  backend. Unknown inclusion is not known completeness.
- Metric instruments aggregate measurements; exemplar sampling does not sample
  the aggregate totals. This does not make a latest Gauge a population rate.
- `sampling.accept: "observed"` explicitly accepts a latest received
  observation. Other coverage/correction requirements are rejected as
  unsupported, not silently downgraded. Record available sampling indicators
  without inventing a probability or learned confidence.

Explicit confirmation and durable decision audit are operational writes, not
sampled telemetry. No sampling decision can create, suppress, or replay an
exposure confirmation on the application's behalf.

## Delivery boundaries and removals

This remains one implementation issue. No prerequisite or graph revision is
needed for the design.

| Issue | Boundary |
| --- | --- |
| #39 | Delivered the reduced activation core independently. Preserve its CAS, replay, and publication guarantees when updating shared input references. |
| #44 | Clean manifest/compiler/client, source-owned inputs, native OTel bindings/materialization, input-only execution seam and necessary null-confidence handling, audit/correlation, runnable examples. |
| #47 | Independently owns server terminology and constraint ownership. #40 consumes that architecture; it is not a prerequisite or scope change for #44. |
| #40 | Consumes those contracts for bundle-approved authority, readiness receipts, normalized weighted-average scoring, exact branch handling, and fixed-default policy re-baseline. |
| #41 | Performs the final bundle-approved Tetris migration and rerun on that completed foundation. |
| #33 / #25 | Temporal contracts and future proposal-managed lifecycle remain separately scoped. |

Do not change #40's scoring arithmetic, reinterpret max-delta, introduce
previous-result history, or build #25's proposal loop here. Retain the accepted
850/750 branches, governed 800 fallback, distinct availability fallback, exact
identity, and one exposure after actual application as downstream invariants.
Tests for not-yet-implemented #40 behavior are not evidence of #44 completion.

Implementation must remove or replace:

- `flaggo-extract`, extractor exports, inline-definition authoring, static AST
  extraction, signal handles, signal schema digests, and `flaggo.signal` emission.
- Global producer declarations, generated signal allowlists, signal-ref input
  arrays, and producer-coupled objective/guardrail references.
- Redundant result/default fields, `onlineStrategy` and `liveInputs`, and
  author-supplied registry identity. Keep one coherent new manifest shape.
- Per-call definition fallback and runtime-client startup registration.
- Implicit numeric-rule evidence/confidence requirements and stale result
  guards that reject the replacement null-confidence contract.

Keep the existing ownership of policy, authority, targets, durable files,
decision idempotency, and exposure confirmation. Do not create compatibility
adapters, migrations, duplicate manifests, or unsupported dormant operators.
Replace existing fixtures and helper scripts instead of preserving old
executable paths. Old persisted formats are rejected explicitly, not migrated
or silently converted. Development/test environments use freshly initialized
storage and publish the new manifest for approval; tooling must not delete a
user's existing data automatically.

The current Tetris/worker integrations must use generated catalogs and explicit
management publication. Replace their Flaggo telemetry-sink usage with ordinary
OTel instrumentation. Add a small local evidence-input scenario rather than
changing the four live Tetris operands to delayed telemetry.

During implementation, align the architecture definition/evidence/runtime
documents, client/shared-contracts/registry/evidence/reasoning/state/audit
designs, SDK and contracts READMEs, contributing boundary guidance, and example
instructions. Keep this issue design linked as the decision rationale; do not
leave contradictory producer-coupled canonical guidance after implementation.

## Acceptance and evidence plan

All evidence below is planned, not yet implemented or verified.

| #44 criterion | Boundary and required evidence |
| --- | --- |
| A1: sole authored manifest and identity | TS/.NET/Python canonical fixtures: input source, selector, meaning, unit, freshness and attribution edits change digest; runtime values/metadata do not; approval/receipt mismatch fails. |
| A2: keys plus runtime data | Generated client tests and migrated examples contain no inline definitions, producer handles, exporter configuration, or extraction entry point. |
| A3: generated typing and service parity | Type tests for unknown/non-number keys, missing context/inputs, wrong primitive; SDK and direct REST negative cases for all source/type/bounds rules. |
| A4: legitimate key-only calls | No-input and global evidence-input fixtures succeed; missing caller/target data fails before transport/fallback; no hidden context/defaults. |
| A5: existing producers end to end | Real OTel emitter -> pinned Collector -> real OTLP host -> durable frame -> evidence-owned input -> numeric result/audit, without `flaggo.signal`. |
| A6: supported signal projections | Gauge, span duration/attribute, named span-event attribute, and structured-log fixtures; invalid unit/type/path/target/timestamp and unsupported sums/histograms/aggregations. |
| A7: ownership and missingness | Reject evidence overrides; missing/stale/future/ambiguous/invalid frames fail explicitly; before/at/after max age; zero is valid only when actually observed. |
| A8: reproducible execution | Spy proves no evidence object crosses executor; audit/exposure contain resolved vector/provenance; an evidence update cannot change a retained decide replay. |
| A9: sampling/coverage honesty | Observed-only acceptance; unknown/biased coverage never becomes population evidence; unsupported correction rejected; exemplar attribution rejected. |
| A10: optional instrumentation/exposure | Core import/use without OTel, compiler, or management initialization; native attributes from confirmation; pending/foreign/unknown IDs rejected; retry yields one exposure. |
| A11: downstream invariants | Preserve accepted numeric/fallback/target outcomes, not old DTOs or fixture assumptions; add input-only/null-confidence proof and explicit policy cases; distinguish #40/#41's remaining re-baseline work. |
| A12: coherent executable change | SDK and REST conformance; stale generated artifact checks; registry/persistence/input-resolution/host tests; migrated worker and real local Tetris harnesses. |
| A13: honest documentation | Canonical documents/examples name implemented projections and operational limits; no claimed rate engine, learned confidence, or completed future authority workflow. |

Additional failure tests cover gzip/decoded-size limits, unauthorized ingest and
cross-scope reads, OTLP response types/rejection counts, concurrent/duplicate/
out-of-order ingestion, metric-series ambiguity, atomic generation reads,
failed durable publication, restart/corruption, capacity limits, and Collector
outage while request-only decisions continue.

Use the existing validation commands and runners during implementation:

```powershell
python tools\dev.py check
npm --prefix packages\sdk-typescript run typecheck
npm --prefix packages\sdk-typescript test
dotnet test tests\Flaggo.Decisioning.Tests\Flaggo.Decisioning.Tests.csproj
npm run test:adaptive-worker
npm run test:tetris-integration
```

Start with affected test selectors, then run the cross-contract and real-host
boundaries. Add the OTel scenario to the existing integration tooling with
bounded startup/waits and owned-process cleanup; do not add a parallel test
framework or require cloud resources.

## Tradeoffs and independent follow-ups

Latest scalar bindings are intentionally less powerful than a telemetry query
language. They establish correct identity, transport, source ownership,
freshness, and attribution before statistics are added. Applications may keep
using their existing observability backend for aggregates; unsupported Flaggo
projections fail explicitly.

Local file snapshots and a single writer are sufficient for this bounded local
slice, not a claim of distributed ingest scalability. Stateful aggregation,
histogram/counter support, statistical correction, durable confirmation
recovery, and multi-writer materialization require separate accepted work.
None blocks request-only client use or silently expands #44.

Outside this repository, adopters author a manifest, consume its generated
catalog/receipt, adjust call sites, and add Collector routing plus appropriate
ingest credentials. They keep their producers/providers and other exporters.
No external repository, cloud service, credential store, or running Collector
is modified by this design action.

## Guidance and sources

Repository guidance: [Manifesto](../MANIFESTO.md) and
[Contributing](../../CONTRIBUTING.md) at the inspected baseline.
Manifest SHA-256:
`5f7e062fbf03ccfa89f302789a4f5284670b73fdab7b31bf193b0beff0bafd77`.
Workspace map SHA-256:
`e8a2c3abec0f249f039b157cdcc756984c94182cfcf30e0c7edec58213b98e2e`.

Protocol references checked for this design:

- [OTLP specification](https://opentelemetry.io/docs/specs/otlp/):
  signal paths, binary/JSON distinction, gzip, response and retry semantics.
- [Upstream OTel Protobuf definitions](https://github.com/open-telemetry/opentelemetry-proto):
  stable protocol definitions and supported C# generation.
- [OTLP HTTP exporter](https://github.com/open-telemetry/opentelemetry-collector/tree/main/exporter/otlphttpexporter):
  `otlp_http`, endpoint suffixes, Protobuf encoding, queueing and compression.
- [Collector configuration](https://opentelemetry.io/docs/collector/configuration/):
  application-owned receivers/processors/exporters and independent pipelines.
- [OTel sampling](https://opentelemetry.io/docs/concepts/sampling/),
  [metrics data model](https://opentelemetry.io/docs/specs/otel/metrics/data-model/),
  and [baggage](https://opentelemetry.io/docs/concepts/signals/baggage/).
