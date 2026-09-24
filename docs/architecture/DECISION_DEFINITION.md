# Decision definition

## Purpose

A decision definition is the versioned semantic contract for a stable key such
as `tetris.dropInterval`. The application authors one JSON manifest; a trusted
publisher submits it to Contract Service. Generated catalogs and approved
receipts bind runtime calls to exact identities. Applications retain their
existing telemetry instrumentation.

The current executable shape is
[`decision-definition-bundle-v2.schema.json`](../../contracts/schemas/decision-definition-bundle-v2.schema.json).
The definition map key is the decision key. One result contract owns type,
action space and safe default without redundant declarations.

## Definition-owned semantics

| Current manifest field | Meaning |
| --- | --- |
| `result` | Primitive type, bounds/allowed values, optional numeric step and one default |
| `context` | Typed caller facts, requiredness and explicit target-ID bindings |
| `targeting` | Permitted hierarchy, primary target and ordered fallback targets |
| `inputs` | Required typed operands with request or evidence ownership |
| `evidence` | Semantic interpretations of existing native OTel sources |
| `intent` | Natural-language intent or numeric objectives over evidence bindings |
| `policy` | Current wire field containing deterministic decision constraints |

Constraint declaration belongs to the definition, validation to Contract
Service, and evaluation to Decision Service. There is no separate Policy
service. #49 migrates executable Policy-named fields/types to
`constraints`/`DecisionConstraints`/`ConstraintEvaluationResult`; the current
field name above describes the schema, not another logical boundary.

Definitions do not own producers, exporters, collection, sampling, telemetry
history, materialized views, active state, or runtime results.
`result`, `targeting` and `policy` are required in the current schema. Omitted
context, inputs and evidence normalize to empty maps.

## One manifest, compact callers

```json
{
  "format": "flaggo.decision-definition-bundle/v2",
  "application": { "id": "game", "environment": "dev" },
  "decisions": {
    "game.speed": {
      "result": { "type": "number", "min": 200, "max": 1500, "step": 50, "default": 800 },
      "context": { "sessionId": { "type": "string", "target": "session", "required": true } },
      "targeting": { "hierarchy": ["session", "global"], "primary": "session", "fallbackOrder": ["global"] },
      "inputs": {
        "pressure": { "source": "request", "type": "number", "range": [0, 1], "meaning": "Current occupied-board fraction." }
      },
      "policy": { "kind": "inline", "constraints": [] }
    }
  }
}
```

After trusted publication, a generated catalog and exact approved receipt
initialize the synchronous client:

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const decision = await flaggo.tune.number("game.speed", {
  context: { sessionId },
  inputs: { pressure }
});
```

There is no per-call definition, runtime registration, producer handle or
implicit telemetry emission. Generated types validate keys, result types and
required caller data, preserving key/request correlation for dynamic unions.
Runtime checks repeat these guarantees for JavaScript. The request argument is
optional only when the contract has no required caller fields.

## Inputs and evidence

Each input has exactly one source:

```json
{
  "pressure": {
    "source": "request", "type": "number", "unit": "1",
    "range": [0, 1], "meaning": "Current queue pressure computed by the application."
  },
  "recentPressure": { "source": "evidence", "binding": "receivedPressure" }
}
```

Every declared input is required. Evidence operands inherit type, meaning,
unit and range from their binding; callers cannot supply or override them.
Request operands need no telemetry instrument or global signal declaration.
Use them for current application state rather than substituting delayed
telemetry.

Bindings select scope, resource attributes, native metric/span/log identity
and optional attributes. They declare supported scalar projection, target,
source-time freshness, observed-only sampling acceptance and attribution.
Required inputs cannot reference confirmed-exposure bindings: the first
decision cannot require its own prior exposure. Those bindings remain valid
for outcome/objective evidence.

Decision Service resolves all evidence operands from one authorized immutable
generation before bounded execution. The executor receives primitive values,
not raw telemetry, export waits or an `EvidenceSnapshot`.
See [Evidence](EVIDENCE.md) and the
[runnable manifest](../../examples/otel-evidence/decision-manifest.json).

Numeric objectives reference numeric bindings with `{ evidence, direction }`.
Only direction `target` accepts a finite target value. Intent guides future
analysis; it does not grant online authority. No producer schema, derived-rate
DSL or arbitrary aggregation language is advertised by this manifest.

## Identity and publication

Contract Service assigns opaque definition lineage and revision.
`{ definitionId, revision, contractDigest }` is the complete runtime identity;
a key never selects an implicit latest server revision.

The digest covers the key and normalized contract. Input ownership/meaning,
binding semantics, target mappings, intent and constraints are semantic.
Map order is irrelevant; ordered fallbacks and body paths are not.
Owner/build/source metadata does not change definition identity.
See [canonical rules](../design/shared-contracts/README.md#canonical-definition-normalization-and-digest).

Trusted management code applies the normalized bundle and obtains exact
approval when required. Runtime accepts only a matching catalog and approved
receipt. No v1 bundle, AST extraction, inline definition or producer
compatibility path remains.

## Targets, constraints and authority

The hierarchy permits target kinds; it does not invent fallback order.
Context target fields are string-typed and unique per kind. Evidence targets
come from declared attributes inside authenticated scope, never resource
attributes treated as authorization.

`result.default` supplies governed fallback and, only when explicitly enabled
and eligible, SDK availability fallback. Their provenance remains distinct.
Constraints may approve, block or require fallback; they never widen, clamp or
repair approved authority. Fixed-default max-delta compares to this default,
not the previous response.

The current reference-policy schema shape is rejected; no implicit resolver
or environment policy exists. #49 removes the unused shape while aligning
executable constraints and record terminology.

The current manifest does not declare initial authority. Trusted local
bootstrap provisions existing State Store authority. #40 owns the extension:

```text
definition + bounded initial authority candidate
  -> exact-snapshot approval
  -> captured stable-head baseline
  -> State Store compare-and-swap
  -> activation-ready receipt
```

An approved-definition receipt is not proof of this future activation. The
manifest cannot supply trusted approval, activation, state or strategy
identities. Future Async Analysis Pipeline candidates use the same Contract
Service boundary, not direct state mutation.

## Durable ownership

Contract Store retains definitions and exact approval facts; State Store owns
activation and immutable authority. Evidence Store owns decision, exposure
and outcome records. These boundaries make behavior reconstructable without
an Audit component; explanations derive from stored facts.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Evidence](EVIDENCE.md)
- [Authority](AUTHORITY.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Shared contracts](../design/shared-contracts/README.md)
- [Contract Service](../design/contract-service/README.md)
- [Client library](../design/client-library/README.md)
