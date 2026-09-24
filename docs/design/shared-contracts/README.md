# Shared contracts design

## Purpose and ownership

`contracts/` owns language-neutral schemas, OpenAPI, fixtures, and semantic
vectors. `packages/shared-contracts` contains shared data records and canonical
utilities, not a global collection of service interfaces. Registry, evidence,
state, policy, reasoning, and audit own their respective ports.

Executable schemas are authoritative:

- [Manifest v2](../../../contracts/schemas/decision-definition-bundle-v2.schema.json)
- [Runtime models](../../../contracts/schemas/runtime-models-v1.schema.json)
- [Management models](../../../contracts/schemas/management-models-v1.schema.json)
- [Problem Details](../../../contracts/schemas/problem-details-v1.schema.json)

The manifest-first replacement rejects the old bundle and input shapes. It
does not add adapters or migrations. HTTP operation paths remain `/v1/...`;
that path is not the manifest format number or a Phase 3 milestone.

## Primitive types and action space

Decision values are finite numbers, booleans, or strings, never null/objects.
Numeric bounds are inclusive, step is positive and relative to `min`, and the
default must be valid. String allowed-values sets are nonempty and unique and
must contain the default. The authored `result` object is the sole result
contract/default; runtime projections may use internal action-space types.

Context contains declared primitive facts and explicit string target-ID
bindings. A hierarchy permits target kinds; only the declared primary and
ordered fallback targets determine resolution.

## Decision contract

```ts
type Manifest = {
  format: "flaggo.decision-definition-bundle/v2";
  application: { id: string; environment: string };
  decisions: Record<string, DecisionDefinition>;
};

type RequestInput = {
  source: "request";
  type: "number" | "boolean" | "string";
  meaning: string;
  // Unit and range are valid only for numeric inputs.
  unit?: string;
  range?: [number, number];
};
type EvidenceInput = { source: "evidence"; binding: string };
```

Each definition owns `result`, required `targeting` and `policy`, optional
context/inputs/evidence maps, intent, and nonsemantic owner metadata.
Every declared input is required. Evidence inputs inherit their binding's
type, unit, range, and meaning; callers cannot override them.

Bindings select supported native Gauge/span/span-event/log observations,
explicit target and latest scalar projection, source-time freshness,
observed-only coverage, and attribution. Numeric objectives reference numeric
evidence bindings. Detailed source constraints belong to the executable schema
and [Evidence](../../architecture/EVIDENCE.md).

No public producer declarations, duplicate default, inline per-call contract,
implicit policy, or initial-authority mode is authored. Policy references are
schema-defined but rejected until a governed resolver exists.

## Runtime API contracts

```text
POST /v1/decisions/{decisionKey}:decide
  expectedContract: { definitionId, revision, contractDigest, ...provenance }
  runtimeContext: declared facts
  inputs: { requestOwnedName: primitiveValue }
  client: { appId, environment, ...SDK metadata }
```

The route supplies the key. Required exact identity, strict JSON, unknown
fields, duplicate properties, input source/type/range, and context/target
coherence are validated. Duplicate properties and obsolete input arrays are
not interpreted as an alternative wire format.

The host supplies verified `ApplicationScope { appId, environment, tenantId }`
from authenticated claims. Tenant identity never comes from body or OTel
resource attributes. Input materialization and confirmation lookup partition
by that authorized scope.

The resolver produces a complete scalar map and per-input provenance from
request values and one pinned evidence generation. Required input evidence
missing/stale/ambiguous/future/invalid/unavailable returns
`503 required-evidence-unavailable`, always local-fallback-ineligible.
Request-only decisions without evidence policy do not access evidence ports.

Server results carry exact definition integrity, result, targets, policy,
fallback, audit/decision identity, and a confirmation directive when applicable.
Deterministic rules report `confidence: null`. SDK availability fallback has
client-only provenance and no server identities.

Decide idempotency is tenant/application/environment scoped. The fingerprint
covers caller content and exact identity, not mutable evidence generations or
evaluation time. Retained successful replay returns the original result.
Confirmation cannot replace decision-time inputs.

## Canonical definition normalization and digest

All implementations normalize the same JSON manifest and use RFC 8785 JSON
Canonicalization Scheme plus SHA-256:

```text
contractDigest = sha256(canonical({ key, contract: normalizedDefinition }))
bundleDigest   = sha256(canonical(normalizedBundle))
```

The key is semantic. Exclude definition owner metadata from `contractDigest`;
retain owner/build/source metadata in the bundle identity. Opaque lineage and
revision IDs are registry outputs, not authored definition fields.

Normalize omitted context/inputs/evidence to empty maps and omitted context
requiredness to false. Apply the shared optional-policy normalization and sort
set-like allowed values/constraints. Object order is immaterial; hierarchy,
fallback order, numeric range endpoints, body paths, and prioritized objectives
retain their meaning and ordering. Reject duplicates rather than choosing the
first or last declaration. Do not synthesize numeric-bounds policy from the
result bounds or infer target precedence.

Input ownership, meaning, type/unit/range, binding source/selectors/projection,
target, freshness, sampling/attribution, result, intent, and policy are semantic.
No runtime value or materialized snapshot participates in a definition digest.
The shared positive/negative
[semantic vectors](../../../contracts/conformance/semantic-digest-vectors-v1.json)
exercise .NET, TypeScript, and Python equivalence.

## Contract identity and integrity

The complete runtime identity is `{ definitionId, revision, contractDigest }`.
The registry preserves an opaque lineage across approved semantic revisions.
Metadata-only changes retain the complete tuple. Clients cannot substitute a
key, bundle digest, or newest revision.

Every successful server result repeats the expected verified identity.
Missing, unknown, conflicting, or retired identity is an explicit error.
Multiple approved revisions can coexist; one revision cannot execute another
revision's governed state or reuse its materialized input frames implicitly.

## Contract bundle and registration receipt

Management validates, classifies, and atomically applies normalized manifests.
New/changed semantics require authenticated exact-snapshot approval with a
captured baseline. Identical/metadata-only application preserves runtime
identity; exact apply replay returns the stored receipt.

The approved receipt binds every manifest key to its exact identity and records
the bundle/build provenance. A generated runtime catalog must match its scope,
bundle digest, exact key set, and semantic digests. Neither the runtime client
nor the separate management helper approves automatically.

This receipt does not claim initial-authority activation. #40 owns that
extension; no temporary authority mode or fabricated activation identity is
present in this manifest.

## Decision strategy, state, and policy

The numeric executor receives only the typed runtime definition, approved
numeric rule, and resolved primitive input map. Rules reference input names,
not producer keys. They do not receive telemetry, lifecycle state, or quality
snapshots and cannot create authority or learned confidence.

Governed state, expected-baseline activation, and stable heads are
[state-owned](../state/README.md). Optional policy-quality evidence is a
separate domain from observed input coverage. It is queried only when the
effective policy requires it. Future proposal-managed authority remains a
separate approved contract, not a request-time agent loop.

## Audit record and exposure

Audit captures authenticated tenant/application/environment, exact identity,
original request inputs, resolved inputs, target/state/policy/fallback details,
reason, and timestamp. Input provenance distinguishes request ownership from
evidence binding, generation, observed nanoseconds, materialization time,
fingerprint, coverage, trace/span IDs/flags, and verified exposure reference.

The decision audit commits before success. Confirmation capabilities never
enter audit. Exposure confirmation prepares stable identity, commits its
durable audit, then commits confirmation. Lookup and replay enforce tenant as
well as application/environment. The snapshot preserves the original resolved
vector/provenance; telemetry cannot rewrite it or stand in for confirmation.
