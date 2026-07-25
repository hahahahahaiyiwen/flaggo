# Contract Registry Design

## Purpose

The contract registry stores the declared meaning of Flaggo decision surfaces.

It owns the server-side resources that application code references at runtime:

- decision surfaces,
- decision contracts,
- action spaces,
- scope hierarchies,
- evidence definitions,
- goals,
- fallback contracts,
- policy references.

The registry exists so runtime decision requests can stay small and governed. Applications should call a pre-registered surface instead of sending all decision semantics inline on every request.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## Design principle

> Treat Flaggo resources as versioned, append-only contracts with lifecycle state, not as things to hard-delete automatically.

Decision resources are part of audit history. Old decisions must remain explainable after code changes, rollbacks, and policy updates.

## Minimal lifecycle states

Keep the lifecycle small at first:

| State | Meaning |
|---|---|
| `active` | Can be used by runtime decision requests. |
| `deprecated` | Still usable for existing clients, but should not be used by new code. |
| `retired` | Not eligible for approved decisions; runtime should return fallback or reject based on policy. Kept for audit/history. |

Avoid hard delete in normal workflows. Hard delete should be exceptional admin-only behavior, if it exists at all.

This simplified lifecycle intentionally skips `draft` and `archived` for the first design. Draft-like behavior can happen in CI before sync. Archived-like behavior can be represented by filtering retired resources out of default UI views.

## Versioning model

Contracts should be append-only revisions.

Example:

```text
tetris.dropInterval@1 active
tetris.dropInterval@2 active
tetris.dropInterval@1 deprecated
tetris.dropInterval@1 retired
```

Changing contract semantics should create a new revision instead of mutating historical meaning in place.

Examples of revision-worthy changes:

- result type changes,
- number range changes,
- string allowed values change,
- scope hierarchy changes,
- fallback contract changes,
- evidence definition changes,
- goal definition changes,
- policy reference changes.

Small metadata changes can be mutable if they do not affect decision semantics, but the first design can keep this conservative.

## Resource ownership manifest

The client SDK or build tooling should produce a resource ownership manifest.

The manifest declares which Flaggo resources are owned by a codebase, app, environment, and source path.

Example shape:

```json
{
  "appId": "tetris-demo",
  "environment": "dev",
  "source": {
    "repository": "tetris-frontend",
    "path": "src/Flaggo",
    "commit": "abc123"
  },
  "surfaces": [
    {
      "name": "tetris.dropInterval",
      "contractRevision": "local",
      "resultType": "number",
      "scopeHierarchy": ["session", "user", "segment", "global"]
    }
  ]
}
```

The manifest lets Flaggo compare declared resources in code with registered resources on the server.

## MVP runtime port

The runtime Decision API should depend on a registry port, not a concrete database or cloud service.

```ts
interface IContractRegistry {
  getActiveContract(ref: DecisionSurfaceRef): Promise<DecisionContract>;
  validateManifest(manifest: ResourceOwnershipManifest): Promise<ManifestValidationResult>;
}

type ManifestValidationResult = {
  result: "valid" | "invalid";
  errors: string[];
  warnings: string[];
};
```

MVP implementation:

- local in-memory registry,
- optional JSON fixture for `tetris.dropInterval`,
- validate-only manifest support,
- no production runtime mutation.

Future implementations can use SQLite, PostgreSQL, cloud SQL, document stores, or object storage behind the same port.

## Sync behavior

Resource sync should classify changes, not blindly overwrite.

```text
code declarations + ownership manifest
  -> compare with registered resources
  -> create new resources/revisions
  -> mark missing resources as deprecation candidates
  -> require explicit retirement
```

Recommended behavior:

| Diff | Action |
|---|---|
| New surface | Create active surface and initial contract revision. |
| Compatible metadata change | Update metadata or create revision based on policy. |
| Semantic contract change | Create a new revision. |
| Resource missing from manifest | Mark as deprecation candidate; do not delete. |
| Deprecated resource with no active clients | Allow explicit retirement. |
| Active runtime usage exists | Block retirement unless forced by operator policy. |

## Missing resources should not auto-delete

If a surface disappears from code, Flaggo should not immediately delete or retire it.

Reasons:

- old deployed clients may still call it,
- rollback may need it,
- audit records reference it,
- operator console needs historical explanations,
- accidental branch changes should not destroy control-plane history.

Default behavior should be:

```text
missing from manifest -> deprecation candidate -> explicit deprecate -> explicit retire
```

## Runtime behavior by lifecycle state

| State | Runtime behavior |
|---|---|
| `active` | Decision API may evaluate and return approved decisions. |
| `deprecated` | Decision API may continue serving existing clients; response may include warning metadata later. |
| `retired` | Decision API should not approve new decisions; return fallback or a controlled error based on policy. |

The first implementation can keep runtime behavior simple:

- `active`: decide normally.
- `deprecated`: decide normally.
- `retired`: fallback.

## Management API implications

The management API should support:

```http
POST /v1/surfaces
GET /v1/surfaces/{surface}
POST /v1/contracts
GET /v1/contracts/{surface}/revisions
POST /v1/manifests:sync
POST /v1/surfaces/{surface}:deprecate
POST /v1/surfaces/{surface}:retire
```

Exact routes can change later. The important design is:

- create/register is explicit,
- sync is manifest-driven,
- semantic updates create revisions,
- deprecation/retirement are lifecycle transitions,
- hard delete is not part of the normal lifecycle.

## Relationship to client library

The runtime client library should not mutate registry resources during normal production runtime.

Recommended flow:

```text
developer writes decision declarations in code
  -> build/CI extracts ownership manifest
  -> Flaggo sync registers or validates resources
  -> deployed app calls runtime Decision API
```

Local development may support `register-dev`, but production should prefer manifest sync or explicit management tooling.

## First slice

For the Tetris hero scenario, the first registry design should support:

- registering `tetris.dropInterval`,
- number result type,
- action space range,
- fallback contract,
- session/user/segment/global hierarchy,
- active/deprecated/retired lifecycle,
- append-only contract revisions,
- ownership manifest sync in validate or create mode.
