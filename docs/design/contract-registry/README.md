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
Phase 1 management-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md#management-api).

## Design principle

> Treat Flaggo resources as versioned, append-only contracts with lifecycle state, not as things to hard-delete automatically.

Decision resources are part of audit history. Old decisions must remain explainable after code changes, rollbacks, and policy updates.

## Minimal lifecycle states

Keep the lifecycle small at first:

| State | Meaning |
|---|---|
| `active` | Can be used by runtime decision requests. |
| `deprecated` | Still usable for existing clients, but should not be used by new code. |
| `retired` | Not eligible for runtime decisions; data plane rejects the retired identity. Kept for audit/history. |

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
  "format": "flaggo.decision-definition-bundle/v1",
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
      "definitionId": "tetris.dropInterval@2",
      "key": "tetris.dropInterval",
      "valueType": "number",
      "actionSpace": {
        "type": "number",
        "min": 200,
        "max": 1500,
        "step": 50,
        "default": 800
      },
      "runtimeContextSchema": {
        "sessionId": { "type": "string", "target": "session" },
        "userId": { "type": "string", "target": "user" },
        "cohort": { "type": "string", "target": "cohort" }
      },
      "targetHierarchy": ["session", "user", "cohort", "global"],
      "inference": {
        "target": "session",
        "inputs": [{ "key": "tetris.boardPressure" }],
        "fallbackOrder": ["cohort", "global"]
      },
      "intent": {
        "type": "metric-objective",
        "primary": { "signal": { "key": "tetris.earlyLossRate24h" }, "direction": "minimize" }
      },
      "fallback": {
        "value": 800,
        "reason": "safe_default_drop_interval"
      },
      "policy": {
        "kind": "inline",
        "constraints": [
          { "kind": "cooldown", "seconds": 20 },
          { "kind": "max-delta", "value": 50 },
          { "kind": "number-bounds", "min": 200, "max": 1500 }
        ]
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

- If semantics are unchanged, it returns the existing definition ID/revision.
- If only metadata changed, it records a metadata revision.
- If semantics changed, apply returns `requires-approval` with an approval request and performs no mutation. Explicit approval atomically creates the new semantic revision and applies the pending bundle.
- Old builds continue using the old identity; new builds use the new identity.
- Within one build, repeated declarations of the same decision key must normalize to the same canonical digest. Identical definitions are deduplicated; different digests are a `contract-conflict` build error.
- Runtime calls attach a build-generated or memoized descriptor and evaluate only bound values. They must not recalculate or register static definition semantics on each call.
- Unsupported or runtime-dependent extraction is invalid. Production runtime returns 4xx Problem Details for an unknown or conflicting identity; it never derives management state from an executed branch, selects another revision, or invokes local fallback.
- Every `inference.inputs` key must resolve to a registered app-emitted primitive metric. Event signals and service-derived metrics are rejected even if a non-TypeScript client submits them.
- Every metric-objective signal must resolve to a registered numeric metric. App-emitted and derived numeric metrics are valid; events and boolean/string metrics are rejected.
- A metric objective with `direction: "target"` must include a finite numeric `target`; `minimize` and `maximize` objectives must not include `target`.
- Every canonical decision definition must contain `policy: PolicyReference | InlinePolicy`. Missing policy is a validation error; the registry does not insert an implicit environment/default reference.
- `schemaDigest` is validated on signal declarations to detect conflicting schemas under one immutable key, but is not copied into `SignalRef` and does not affect decision-definition identity.

The registry and SDK use the canonicalization rules in [Shared Contracts](../shared-contracts/README.md#canonical-definition-normalization-and-digest), including RFC 8785 serialization, order-sensitive hierarchy/precedence arrays, key-sorted signal-role sets, and duplicate rejection.

## MVP runtime port

The runtime Decision API should depend on a registry port, not a concrete database or cloud service.

```ts
interface IDefinitionRegistry {
  getActiveDefinition(ref: DecisionDefinitionRef): Promise<DecisionDefinition>;
  validateBundle(bundle: DecisionDefinitionBundle): Promise<DefinitionBundleValidationResult>;
  applyBundle(bundle: DecisionDefinitionBundle): Promise<DefinitionBundleApplyResult>;
}

interface IDefinitionBundleApprovalStore {
  getApproval(approvalRequestId: string): Promise<DefinitionBundleApprovalResult>;
  approve(approvalRequestId: string): Promise<DefinitionBundleApprovalResult>;
  reject(approvalRequestId: string): Promise<DefinitionBundleApprovalResult>;
}

type DefinitionBundleValidationResult = {
  status: "valid" | "invalid";
  bundleDigest?: string;
  contractDigest?: string;
  compatibility?: ContractCompatibility;
  issues: ContractIssue[];
};

type ContractIssue = {
  code: string;
  severity: "error" | "warning";
  path: string;
  message: string;
  decisionKey?: string;
  signalKey?: string;
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
| `retired` | Decision API rejects runtime use with a contract/configuration error. |

The first implementation can keep runtime behavior simple:

- `active`: decide normally.
- `deprecated`: decide normally.
- `retired`: return `409 retired-definition`; do not select another revision or local fallback.

## Management API implications

The management API should support:

```http
POST /v1/definition-bundles:validate
POST /v1/definition-bundles:apply
GET /v1/definition-bundle-approvals/{approvalRequestId}
POST /v1/definition-bundle-approvals/{approvalRequestId}:approve
POST /v1/definition-bundle-approvals/{approvalRequestId}:reject
```

Bundle validation/application and semantic-revision approval are the blocking Phase 1 management contract. Definition reads and explicit deprecate/retire operations remain necessary management capabilities, but their routes can be designed after the runtime/client integration seam is unblocked.

The important design is:

- create/register is explicit,
- sync is bundle-driven,
- validation is read-only and returns structured issues,
- apply repeats validation and is atomic and idempotent,
- semantic change returns `requires-approval` plus a stable `approvalRequestId` without mutation,
- approval atomically applies the pending canonical bundle and stores its approved receipt,
- approval status returns `pending`, `approved`, or `rejected`, and includes the receipt only when approved,
- startup retry with the same bundle returns the final approved receipt after approval,
- registration returns a receipt with bundle digest, definition digest, build metadata, and per-decision-key revisions,
- semantic updates create revisions,
- deprecation/retirement are lifecycle transitions,
- hard delete is not part of the normal lifecycle.

Bundle atomicity is all-or-nothing (A5). Semantic revision creation requires explicit approval with no pre-approval mutation (A6).

## Relationship to client library

The runtime client library should not mutate registry resources during normal production runtime.

Recommended flow:

```text
developer writes decision declarations in code, JSON/YAML, or registry UI
  -> static extraction creates or selects DecisionDefinitionBundle
  -> application deployment proceeds independently
  -> trusted application/bootstrap startup validates/applies bundle
  -> Flaggo returns receipt and compact runtime binding
  -> data-plane client is initialized
  -> runtime call succeeds only for that exact registered identity
```

MVP uses explicit startup registration as its first control-plane client. Future modes can include local-only, startup verify-only, manual CLI, release automation, GitOps, init/deployment hooks, and operator or registry-first workflows. Polari does not block external application deployment; failed startup apply produces no accepted identity, leaves the data-plane client disabled, and never selects an older revision or fallback.

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
