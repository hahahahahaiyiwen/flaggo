# Decision definition

## Purpose

A decision definition is the semantic contract for a stable application key,
such as `tetris.dropInterval`. The application authors one JSON manifest;
publication and runtime catalogs are generated from it. Adopting Flaggo does
not require replacing the application's telemetry instrumentation.

The executable contract is
[`decision-definition-bundle-v2.schema.json`](../../contracts/schemas/decision-definition-bundle-v2.schema.json).
The definition map key is the decision key. A definition does not repeat its
key, result type, or default in several places.

## Ownership

| Field | Owns |
| --- | --- |
| `result` | Primitive type, bounds/allowed values, optional numeric step, and one safe default |
| `context` | Typed request metadata, required fields, and target-ID bindings |
| `targeting` | Allowed hierarchy, primary target, and explicit ordered fallback targets |
| `inputs` | Required, decision-local operands with request or evidence ownership |
| `evidence` | Semantic interpretations and projections of existing native OTel sources |
| `intent` | Natural-language intent or numeric objectives over evidence bindings |
| `policy` | Explicit safety constraints or a governed policy reference |

Definitions do not own instrumentation, exporters, collection, sampling, raw
telemetry history, snapshots, approved active state, or runtime results.
`result`, `targeting`, and `policy` are required. Omitted context, inputs, and
evidence normalize to empty maps; they do not create implicit operands.

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

After trusted publication, the generated catalog and exact approved receipt
initialize a synchronous client:

```ts
const flaggo = createFlaggoClient({ catalog, receipt, dataPlaneUrl, dataPlaneCredential });
const decision = await flaggo.tune.number("game.speed", {
  context: { sessionId },
  inputs: { pressure }
});
```

There is no per-call definition, runtime registration, producer handle, or
implicit telemetry emission. Generated TypeScript checks keys, result types,
required context, and request inputs. Runtime checks repeat those guarantees
for JavaScript and dynamic values. The argument can be omitted only if no
required caller fields exist.

## Inputs and evidence

An input has exactly one owner:

```json
{
  "pressure": {
    "source": "request", "type": "number", "unit": "1",
    "range": [0, 1], "meaning": "Current queue pressure computed by the application."
  },
  "recentPressure": { "source": "evidence", "binding": "receivedPressure" }
}
```

Every declared input is required. Evidence inputs inherit type, meaning,
units, and bounds from their binding. Callers cannot supply or override them.
Request inputs need no telemetry instrument or global metric identity.
Use request inputs for facts that must reflect current application state;
delayed telemetry is not a replacement for live state.

Bindings select exact instrumentation scope, resource attributes, native
metric/span/log identity, and optional record attributes. They declare a
supported scalar projection, target, source-time freshness, observed-only
sampling acceptance, and attribution. See [Evidence](EVIDENCE.md) and the
[runnable OTel manifest](../../examples/otel-evidence/decision-manifest.json).

Numeric objectives reference numeric evidence bindings using
`{ evidence, direction }`, with a finite `target` only for direction `target`.
No rate/aggregation DSL or producer schema is declared in this manifest.

## Identity and publication

The registry assigns opaque `definitionId` and `revision`. The complete
runtime identity is `{ definitionId, revision, contractDigest }`; a key alone
never requests the latest server revision.

The semantic digest covers the decision key and its normalized contract.
Input ownership, meaning, evidence selectors, projection, freshness, sampling,
targeting, intent, and policy are semantic. Map order is irrelevant; ordered
fallbacks and body paths are not. Owner/build/source metadata does not change
the definition digest. See the
[canonical rules](../design/shared-contracts/README.md#canonical-definition-normalization-and-digest).

Trusted management code applies the normalized bundle and obtains explicit
approval when required. The runtime client accepts only the matching catalog
and approved receipt. No v1 bundle, inline authoring, or producer compatibility
path remains.

## Targets, policy, and authority

The hierarchy permits target kinds; it does not invent fallback order. Context
target bindings are string-typed and unique per target kind. Evidence target
IDs come from declared attributes, never from arbitrary telemetry scope claims.

The one `result.default` supplies governed fallback and, only when explicitly
enabled and eligible, SDK availability fallback. Those outcomes keep distinct
provenance. Policy references are schema-defined but unresolved references are
rejected by the current registry; there is no implicit policy catalog.

The manifest currently does not declare initial authority. Existing governed
state remains a separate state-owned contract, and local examples provision it
through trusted fixtures. [#40](https://github.com/hahahahahaiyiwen/flaggo/issues/40)
owns the initial-authority and ready-after-activation extension. An approved
definition receipt must not be represented as proof of that future activation.

## Related documents

- [Architecture overview](OVERVIEW.md)
- [Evidence](EVIDENCE.md)
- [Runtime execution](RUNTIME_EXECUTION.md)
- [Client library](../design/client-library/README.md)
- [Tetris scenario](../scenarios/TETRIS.md)
