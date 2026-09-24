# Contract registry design

## Purpose

The registry owns approved manifest semantics, exact runtime identity,
immutable approval snapshots, and typed projections. It does not own telemetry
instrumentation, raw observations, or active authority.

The only authoring input is the normalized
[`flaggo.decision-definition-bundle/v2`](../../../contracts/schemas/decision-definition-bundle-v2.schema.json)
manifest. A decisions map replaces inline application declarations and global
producer schemas. Every definition owns one result/default, explicit
targeting/policy, typed request/evidence inputs, native OTel bindings, and intent.

## Validation

Validation is read-only; apply repeats it. Strict JSON/schema checks precede
semantic projection. Stable issues contain codes, severity, and JSON Pointers.
The registry rejects:

- duplicate/unknown fields, obsolete bundle shapes, invalid result/defaults;
- missing/invalid targeting, repeated target bindings, undeclared fallback kinds;
- missing input meaning, invalid type/range, unresolved evidence references;
- unsupported native projections, invalid source/target/attribution combinations,
  freshness overflow, and objectives over nonnumeric evidence;
- absent/invalid policy or unresolved policy references.

There is no implicit hierarchy, policy catalog, derived metric language, or
initial-authority mode. A definition cannot approve itself.

## Canonical identity

The digest covers `{ key, contract: normalizedDefinition }` using the
[shared normalization rules](../shared-contracts/README.md#canonical-definition-normalization-and-digest).
Input meaning/ownership and all executable evidence semantics participate.
Owner/build/source metadata remains publication provenance, not definition
semantics.

| Identity | Meaning |
| --- | --- |
| Decision key | Stable developer-facing family within application/environment |
| `definitionId` | Opaque registry-issued lineage |
| `revision` | Opaque approved semantic revision |
| `contractDigest` | Canonical key and contract identity |
| `bundleDigest` | Complete normalized publication including metadata |
| Build/deployment metadata | Workload provenance, not implicit contract selection |

Metadata-only changes retain the exact tuple. Approved semantic changes create
a new revision under the lineage. Runtime always requests the complete tuple;
neither latest nor older revisions are selected implicitly.

## Typed definition read ports

Consumers do not parse approval/persistence JSON:

| Port | Projection |
| --- | --- |
| `IRuntimeDefinitionReader` | Exact identity, typed result/default, context/targeting, request/evidence input contracts, bindings, runtime policy |
| `IIntelligenceDefinitionReader` | Same identity, typed objectives/evidence, action space and safety envelope |
| `IEvidenceBindingReader` | Approved, non-retired bindings for authorized application/environment, carrying the host-verified scope |

Projection construction is atomic. Registry lookup remains application/
environment based; materialized frames additionally partition by authenticated
tenant. The binding reader does not authorize tenant claims from telemetry.
No producer roles or workflow permission flags are fabricated.

## Publication and approval

```text
manifest -> read-only validate -> apply/classify
  identical or metadata-only -> stored receipt
  new/changed semantics -> immutable pending approval
  authorized exact-snapshot approval + unchanged baseline -> publish + receipt
```

Apply is atomic and idempotent by canonical bundle and key. Reusing a key for a
different bundle conflicts. Approval compares the expected digest and captured
active-definition baseline, so a stale reviewer cannot replace intervening
work. Review exposes canonical snapshot/diff and prior/proposed digests.

Approve/reject are replay-safe terminal operations. Opposite terminal actions
conflict, expiry is authoritative, and retrying an expired apply creates one
fresh linked request after revalidation. Approved apply replay returns the
same stored receipt, including after restart.

Supported operations remain validate/apply, approval read/snapshot, approve,
and reject. Explicit deprecation/retirement routes are not introduced here.
Missing resources never cause automatic deletion of historical contracts.
`active` and `deprecated` exact identities may execute; `retired` is rejected.

The current receipt proves approved definition publication. Initial-authority
declaration/activation and stronger readiness receipts are #40, not an implied
part of manifest v2. Local examples provision governed state separately in a
trusted harness.

## Persistence and failure

`LocalFileDefinitionRegistry` holds an exclusive cross-process lease, reloads
state for each operation, and publishes mutations atomically. Stored state
includes both projections, apply outcomes, pending snapshots, captured
baselines, expiry, and terminal decisions.

Persistence version 2 is strict. Older payloads, corruption, inaccessible
storage, and lock starvation fail explicitly. No migration, seeded-field
repair, guessed targeting, or fallback to seeded state occurs. Changes remain
visible to running data-plane processes without domain-layer file access.

## Validation evidence

Registry tests cover manifest semantics, semantic vectors, approval conflicts
and replay, metadata-only identity, typed projections, exact scope filtering,
restart, unsupported persistence, and strict malformed-input rejection.
The compiler and .NET/Python gates consume the same executable contracts.
