# Contract Registry Design

## Purpose

The contract registry stores the declared meaning of Flaggo decision definitions.

It owns the server-side resources that application code references at runtime:

- stable decision keys,
- versioned decision definitions,
- action spaces,
- supported runtime target kinds,
- allowed control target kinds,
- signal role references and evidence view requirements,
- goals,
- fallback contracts,
- policy references.

The registry exists so runtime decision requests can stay small and governed. Applications should call a pre-registered decision key and expected definition identity instead of sending all decision semantics inline on every request.

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

Decision definitions should be append-only semantic revisions behind a stable decision key.

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
- supported target or control target changes,
- fallback contract changes,
- signal role/reference changes,
- goal definition changes,
- policy reference changes.

Small metadata changes can be mutable if they do not affect decision semantics, but the first design can keep this conservative. The developer-facing key can stay stable while the registry manages semantic revisions.

## Decision definition bundle

The Contract Registry accepts a canonical, language-neutral `DecisionDefinitionBundle`.

SDK extraction, hand-authored JSON/YAML, GitOps workflows, and registry-first tooling should all produce or reference this same bundle shape. The bundle declares which Flaggo resources are owned by a codebase, app, environment, and source path.

Example shape:

```json
{
  "format": "flaggo.contract-bundle/v1",
  "application": {
    "id": "tetris-demo",
    "environment": "dev"
  },
  "build": {
    "buildId": "tetris-web-2026-07-25.1",
    "artifactDigest": "sha256:container..."
  },
  "source": {
    "repository": "tetris-frontend",
    "path": "src/Flaggo",
    "commit": "abc123"
  },
  "definitions": [
    {
      "key": "tetris.dropInterval",
      "valueType": "number",
      "targetHierarchy": ["session", "user", "cohort", "global"],
      "inference": {
        "target": "session",
        "inputs": [{ "key": "tetris.boardPressure" }],
        "fallbackOrder": ["cohort", "global"]
      },
      "intent": {
        "type": "metric-objective",
        "primary": { "signal": { "key": "tetris.earlyLossRate24h" }, "direction": "minimize" }
      }
    }
  ]
}
```

The bundle lets Flaggo compare declared resources with registered resources on the server without depending on application source code or a language SDK.

## Build and deployment identity

Production may run multiple builds of the same service at the same time. Each build may have:

- identical contract definitions,
- metadata-only differences,
- semantic contract conflicts that require a new ID or revision,
- different telemetry or outcome declarations,
- different source commits or container artifacts.

The registry should therefore distinguish:

| Identity | Meaning |
| --- | --- |
| `contractDigest` | Hash of canonical compatibility-critical contract content. Shared by different builds when their executable contract is equivalent. |
| `bundleDigest` | Hash of the submitted bundle artifact, including non-semantic metadata when appropriate. |
| `buildId` | Human-readable build/release identifier. |
| `artifactDigest` | Container, package, or binary digest. |
| `deploymentId` | Runtime deployment or rollout identifier. |

The runtime Decision API should verify the definition identity provided by the calling workload. It should not assume there is only one active definition bundle per application/environment.

Simpler MVP rule:

> A definition ID is a semantic boundary. Metadata-only changes can keep the same ID. Semantic changes that affect output contract, target hierarchy, signal meaning, outcome meaning, or safety policy must produce a new definition revision or ID instead of mutating the old definition in place.

The preferred UX is registry-managed versioning. Developers can keep writing:

```ts
flaggo.tune.number("tetris.dropInterval", {
  targetHierarchy: ["session", "user", "cohort", "global"],
  signals: {
    evidence: [
      piecePlacedEvent,
      sessionEndedEvent,
      earlyLossRateSignal
    ]
  },
  intent: {
    type: "metric-objective",
    primary: { signal: earlyLossRateSignal, direction: "minimize" }
  },
  inference: {
    target: "session",
    inputs: [boardPressureSignal.input(boardPressure)],
    fallbackOrder: ["cohort", "global"]
  },
  output: {
    default: 800,
    range: [200, 1500]
  },
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minEvidenceQuality: 0.7,
    maxModelUncertainty: 0.35
  },
  context: {
    sessionId: flaggo.target.session(sessionId),
    userId: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort)
  }
});
```

Tooling extracts signal identities and target schemas from the bindings above, discards their runtime values, and sends only canonical definition semantics to the registry. During validation/apply, the registry compares those submitted semantics with the existing contract:

- If semantics are unchanged, it returns the existing contract ID/revision.
- If only metadata changed, it records a metadata revision.
- If semantics changed, it rejects auto-overwrite and either asks for approval to mint a new semantic revision/ID or returns a suggested ID such as `tetris.dropInterval@2`.
- Old builds continue using the old identity; new builds use the new identity.
- Within one build, repeated declarations of the same decision key must normalize to the same canonical digest. Identical definitions are deduplicated; different digests are a `contract-conflict` build error.
- Runtime calls attach a build-generated or memoized descriptor and evaluate only bound values. They must not recalculate or register static definition semantics on each call.
- Unsupported or runtime-dependent extraction is invalid. Production runtime must fall back for an unknown/conflicting identity rather than deriving management state from an executed branch.

The registry and SDK use the canonicalization rules in [Shared Contracts](../shared-contracts/README.md#canonical-definition-normalization-and-digest), including RFC 8785 serialization, order-sensitive hierarchy/precedence arrays, key-sorted signal-role sets, and duplicate rejection.

## MVP runtime port

The runtime Decision API should depend on a registry port, not a concrete database or cloud service.

```ts
interface IDefinitionRegistry {
  getActiveDefinition(ref: DecisionDefinitionRef): Promise<DecisionDefinition>;
  validateBundle(bundle: DecisionDefinitionBundle): Promise<DefinitionBundleValidationResult>;
  applyBundle(bundle: DecisionDefinitionBundle): Promise<RegistrationReceipt>;
}

type DefinitionBundleValidationResult = {
  result: "valid" | "invalid";
  bundleDigest?: string;
  contractDigest?: string;
  errors: string[];
  warnings: string[];
};
```

MVP implementation:

- local in-memory registry,
- optional JSON fixture for `tetris.dropInterval`,
- validate/apply support for `DecisionDefinitionBundle`,
- stable bundle and definition digest generation,
- registration receipt output,
- no production runtime mutation.

Future implementations can use SQLite, PostgreSQL, cloud SQL, document stores, or object storage behind the same port.

## Sync behavior

Bundle sync should classify changes, not blindly overwrite.

```text
code declarations, hand-authored bundle, or registry export
  -> compare with registered resources
  -> classify semantic changes
  -> keep existing IDs for unchanged contracts
  -> create new semantic IDs/revisions for conflicts
  -> mark missing resources as deprecation candidates
  -> require explicit retirement
  -> return registration receipt
```

If multiple builds are live, each runtime request carries the expected bundle/definition identity for that workload. The registry should recognize known immutable definition IDs/revisions rather than forcing all builds onto one current definition.

Recommended behavior:

| Diff | Action |
|---|---|
| New decision key | Create active decision definition and initial revision. |
| Compatible metadata change | Update metadata or create revision based on policy. |
| Semantic definition change | Reject overwrite; create or require a new semantic definition ID/revision. |
| Resource missing from bundle | Mark as deprecation candidate; do not delete. |
| Deprecated resource with no active clients | Allow explicit retirement. |
| Active runtime usage exists | Block retirement unless forced by operator policy. |

## Missing resources should not auto-delete

If a decision key disappears from code, Flaggo should not immediately delete or retire it.

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
POST /v1/definitions
GET /v1/definitions/{decisionKey}
GET /v1/definitions/{decisionKey}/revisions
POST /v1/definition-bundles:validate
POST /v1/definition-bundles:apply
POST /v1/definitions/{decisionKey}:deprecate
POST /v1/definitions/{decisionKey}:retire
```

Exact routes can change later. The important design is:

- create/register is explicit,
- sync is bundle-driven,
- registration returns a receipt with bundle digest, definition digest, build metadata, and per-decision-key revisions,
- semantic updates create revisions,
- deprecation/retirement are lifecycle transitions,
- hard delete is not part of the normal lifecycle.

## Relationship to client library

The runtime client library should not mutate registry resources during normal production runtime.

Recommended flow:

```text
developer writes decision declarations in code, JSON/YAML, or registry UI
  -> build/CI/release creates or selects DecisionDefinitionBundle
  -> Flaggo validates/applies bundle and returns receipt
  -> deployment carries expected definition identity for that build
  -> deployed app calls runtime Decision API
```

Local development may support local-only or explicit local registration workflows, but production should prefer bundle validation/application through explicit management, release, or GitOps tooling.

## First slice

For the Tetris hero scenario, the first registry design should support:

- registering `tetris.dropInterval`,
- number result type,
- action space range,
- fallback contract,
- session/user/cohort/global hierarchy,
- active/deprecated/retired lifecycle,
- append-only definition revisions,
- definition bundle validate/apply flow,
- registration receipt with digest and revision.
