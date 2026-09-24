# Shared contracts design

## Purpose and ownership

`contracts/` owns language-neutral schemas, OpenAPI, fixtures and semantic
vectors. `packages/shared-contracts` contains shared data records and canonical
utilities, not a global interface collection. Ports stay beside the owning
Contract Service, Decision Service, ingestion, analysis or store capability.
Current libraries implement these logical boundaries; they are not additional
server components.

- [Manifest v2](../../../contracts/schemas/decision-definition-bundle-v2.schema.json)
- [Runtime models](../../../contracts/schemas/runtime-models-v1.schema.json)
- [Management models](../../../contracts/schemas/management-models-v1.schema.json)
- [Problem Details](../../../contracts/schemas/problem-details-v1.schema.json)

Executable shapes are authoritative. #49 migrates Policy/Audit-named fields
and libraries to definition constraints and Evidence Store records, without
compatibility aliases. #40 subsequently adds initial authority and
activation-ready receipts. HTTP `/v1/` and manifest format version are
independent.

## Values, targets and manifest

Values are finite, lossless numbers, booleans or strings, never null/objects.
Numeric bounds are inclusive; step is positive relative to `min`; the one
`result.default` must be valid. String allowed-value sets are nonempty and
unique and contain the default.

Context declares primitive facts and explicit string target-ID bindings.
Hierarchy permits target kinds, while primary and ordered fallback targets
determine state resolution.

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
  unit?: string;
  range?: [number, number];
};
type EvidenceInput = { source: "evidence"; binding: string };
```

Unit/range apply only to numeric request operands. Every operand is required.
Evidence operands inherit binding type/unit/range/meaning and cannot be
supplied by callers. Runtime input bindings require attribution `none`;
confirmed-exposure bindings remain valid for outcome/objective evidence.

Definitions own result, targeting, current `policy` constraint data, optional
context/input/evidence maps, intent and nonsemantic owner metadata. Native
bindings select Gauge/span/span-event/log scalars, exact target, latest
projection, source freshness and observed coverage.
Numeric objectives reference numeric bindings. No global producer declaration,
derived-rate language, redundant default or inline call-site contract exists.
The current reference-policy shape is rejected, not silently resolved.

## Runtime contract and input ownership

```text
POST /v1/decisions/{decisionKey}:decide
  expectedContract: { definitionId, revision, contractDigest, ...provenance }
  runtimeContext: declared facts
  inputs: { requestOwnedName: primitiveValue }
  client: { appId, environment, ...SDK metadata }
```

Strict parsing rejects duplicate properties, unknown closed-schema fields,
obsolete input arrays, invalid primitives and incoherent context/targets.
Verified `ApplicationScope { appId, environment, tenantId }` comes from
authentication, never caller JSON or resource attributes.

Decision Service resolves a primitive map and per-input provenance from
request values and one pinned evidence generation. Missing/stale/future/
ambiguous/invalid/unavailable required inputs return 503
`required-evidence-unavailable`, always SDK-fallback-ineligible. Request-only
decisions without evidence-dependent constraints access neither evidence port.

Current server results contain exact verified identity, value, targets,
`policy` evaluation, fallback, `auditId`/decision identity and exposure
directive. These schema field names do not create standalone Policy/Audit
components. Numeric rules have null learned confidence. SDK availability
fallback has client-only provenance and no server identities.

Decide retry identity partitions authenticated tenant/app/environment and
canonical caller content. It excludes mutable materialization generations and
evaluation time. Retained success keeps the original resolved result;
confirmation accepts no replacement input vector.

## Canonical definition normalization and digest

All implementations use RFC 8785 canonical JSON and SHA-256:

```text
contractDigest = sha256(canonical({ key, contract: normalizedDefinition }))
bundleDigest   = sha256(canonical(normalizedBundle))
```

The key is semantic. Definition owner metadata is excluded from the contract
digest but retained, with build/source metadata, in bundle identity. Opaque
lineage and revision are server outputs, never author-supplied identity.

Omitted context/input/evidence maps normalize to empty maps; omitted context
requiredness becomes false. Shared optional-policy normalization and
set-like allowed-value/constraint sorting apply. Object order is immaterial;
hierarchy, fallback order, ranges, body paths and prioritized objectives retain
order/meaning. Duplicates are rejected, not selected first/last. No synthetic
numeric-bound constraint or implicit target order is introduced.

Input ownership/type/unit/range/meaning, binding selectors/projection/target/
freshness/sampling/attribution, result, intent and constraints are semantic.
Runtime values and materialized snapshots never participate in definition
identity. [Shared vectors](../../../contracts/conformance/semantic-digest-vectors-v1.json)
exercise .NET, TypeScript and Python identity and semantic validation.

## Identity, approval and receipt

The complete runtime tuple is `{ definitionId, revision, contractDigest }`.
Approved semantic changes retain lineage and create a new revision;
metadata-only changes preserve the tuple. A key/bundle digest/newest revision
cannot substitute for it. One revision cannot silently execute another
revision's state or inherit its input frames.

Contract Service validates/classifies and atomically applies a manifest.
New/changed semantics require authenticated exact-snapshot approval against a
captured baseline. Exact apply replay returns the stored approved receipt.

The receipt binds every key to exact identity and bundle/build provenance.
Runtime catalog scope, bundle digest, keys and contract digests must match.
Neither runtime initialization nor the management helper approves itself.
Current receipts are approved-definition bindings; #40's stronger
activation-converged receipt is not fabricated by this schema.

## State, execution and constraints

The numeric executor receives only definition, approved rule and resolved
primitive inputs. Rule references are input names, not producer keys. It sees
no lifecycle state/telemetry/quality snapshot and cannot select fallback,
invent confidence or create authority.

[State Store](../state-store/README.md) owns stable authority heads,
immutable payloads, expected-baseline CAS, replay and lineage. Optional
constraint-quality evidence is distinct from observed input coverage.
Async candidates belong to [Async Analysis Pipeline](../async-analysis/README.md)
and require Contract Service approval, never an online agent loop.

## Decision records and exposure

Evidence Store owns reconstructable decision/exposure/outcome records.
The current append adapter records scope, exact identity, caller/resolved
inputs, target/strategy/constraint/fallback facts, reason and timestamp.
Evidence input provenance retains binding, generation, source nanoseconds,
materialization time, fingerprint, coverage, trace/span/flags, verified
exposure and actual evidence-target resolution/source/claim.

Durable decision append precedes success. Confirmation capabilities never
enter record output. Exposure preparation reserves identity, durable append
precedes final commit, and retry preserves the same exposure under exact
scope. A pending or audit-only preparation is not a completed confirmation.
Current confirmation lookup is in-memory; new references to lost confirmations
fail closed while retained validated frames preserve provenance.

Complete authority-lineage record integration remains downstream. See
[Evidence Store](../evidence-store/README.md) for current persistence details
and the distinction from its target record model.
