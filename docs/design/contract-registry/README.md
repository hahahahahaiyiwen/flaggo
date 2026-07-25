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

## Contract bundle

The Contract Registry accepts a canonical, language-neutral `ContractBundle`.

SDK extraction, hand-authored JSON/YAML, GitOps workflows, and registry-first tooling should all produce or reference this same bundle shape. The bundle declares which Flaggo resources are owned by a codebase, app, environment, and source path.

Example shape:

```json
{
  "format": "flaggo.contract-bundle/v1",
  "application": {
    "id": "tetris-demo",
    "environment": "dev"
  },
  "source": {
    "repository": "tetris-frontend",
    "path": "src/Flaggo",
    "commit": "abc123"
  },
  "surfaces": [
    {
      "name": "tetris.dropInterval",
      "valueType": "number",
      "scopeHierarchy": ["session", "user", "segment", "global"]
    }
  ]
}
```

The bundle lets Flaggo compare declared resources with registered resources on the server without depending on application source code or a language SDK.

## MVP runtime port

The runtime Decision API should depend on a registry port, not a concrete database or cloud service.

```ts
interface IContractRegistry {
  getActiveContract(ref: DecisionSurfaceRef): Promise<DecisionContract>;
  validateBundle(bundle: ContractBundle): Promise<ContractBundleValidationResult>;
  applyBundle(bundle: ContractBundle): Promise<RegistrationReceipt>;
}

type ContractBundleValidationResult = {
  result: "valid" | "invalid";
  digest?: string;
  errors: string[];
  warnings: string[];
};
```

MVP implementation:

- local in-memory registry,
- optional JSON fixture for `tetris.dropInterval`,
- validate/apply support for `ContractBundle`,
- stable digest generation,
- registration receipt output,
- no production runtime mutation.

Future implementations can use SQLite, PostgreSQL, cloud SQL, document stores, or object storage behind the same port.

## Sync behavior

Bundle sync should classify changes, not blindly overwrite.

```text
code declarations, hand-authored bundle, or registry export
  -> compare with registered resources
  -> classify compatibility
  -> create new resources/revisions
  -> mark missing resources as deprecation candidates
  -> require explicit retirement
  -> return registration receipt
```

Recommended behavior:

| Diff | Action |
|---|---|
| New surface | Create active surface and initial contract revision. |
| Compatible metadata change | Update metadata or create revision based on policy. |
| Semantic contract change | Create a new revision. |
| Resource missing from bundle | Mark as deprecation candidate; do not delete. |
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
missing from bundle -> deprecation candidate -> explicit deprecate -> explicit retire
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
POST /v1/contracts/bundles:validate
POST /v1/contracts/bundles:apply
POST /v1/surfaces/{surface}:deprecate
POST /v1/surfaces/{surface}:retire
```

Exact routes can change later. The important design is:

- create/register is explicit,
- sync is bundle-driven,
- registration returns a receipt with digest and revision,
- semantic updates create revisions,
- deprecation/retirement are lifecycle transitions,
- hard delete is not part of the normal lifecycle.

## Relationship to client library

The runtime client library should not mutate registry resources during normal production runtime.

Recommended flow:

```text
developer writes decision declarations in code, JSON/YAML, or registry UI
  -> build/CI/release creates or selects ContractBundle
  -> Flaggo validates/applies bundle and returns receipt
  -> deployment carries expected digest/revision
  -> deployed app calls runtime Decision API
```

Local development may support local-only or explicit local registration workflows, but production should prefer bundle validation/application through explicit management, release, or GitOps tooling.

## First slice

For the Tetris hero scenario, the first registry design should support:

- registering `tetris.dropInterval`,
- number result type,
- action space range,
- fallback contract,
- session/user/segment/global hierarchy,
- active/deprecated/retired lifecycle,
- append-only contract revisions,
- contract bundle validate/apply flow,
- registration receipt with digest and revision.
