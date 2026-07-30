# Control Plane and Data Plane UX

## Purpose

Polari follows a cloud-service control-plane/data-plane model while preserving code-first authoring.

Code-first means application code can be the source of decision definitions. Control-plane and data-plane APIs remain separate even when one SDK coordinates both during application bootstrap. Polari does not become responsible for deploying application code.

## Responsibility boundary

| Plane | Responsibility | Typical caller |
| --- | --- | --- |
| Control plane | Validate and apply definition bundles; manage immutable definitions, revisions, policies, strategies, and lifecycle. | Application bootstrap SDK, developer, CLI, release tooling, GitOps, or operator workflow |
| Data plane | Evaluate a pre-registered expected definition and confirm exposure. | Running application or direct REST client |
| Application deployment | Build and deploy application code. | Developer-owned deployment system |

Application deployment and definition registration are independent operations. For the MVP, the default code-first experience registers during trusted application/bootstrap startup after deployment and before data-plane use. If startup registration is skipped or fails, the application can still run, but Polari decision calls cannot.

Polari can later provide manual, CI/CD, GitOps, init-container, sidecar, and registry-first control-plane clients without changing the service boundary.

## Code-first lifecycle

```text
development
  developer writes flaggo.tune.number(...)
  tooling can extract a canonical DecisionDefinitionBundle

application deployment
  developer deploys code independently

application/bootstrap startup (MVP)
  SDK loads the extracted canonical bundle
  SDK calls control-plane validate/apply
  registry returns definitionId + revision + contractDigest
  SDK initializes the data-plane client with that binding

data plane
  application calls decide with the exact expected contract identity
  Decision API evaluates only registered definitions
```

## Control-plane client experiences

The architecture supports several experiences:

| Experience | Behavior | Initial status |
| --- | --- | --- |
| Application/bootstrap startup registration | Trusted startup code atomically applies the extracted bundle and initializes runtime identity before data-plane use. | **MVP default** |
| Startup verify-only | Startup verifies a pre-published identity without management mutation. | Future |
| Manual CLI | Developer explicitly extracts, validates, and applies definitions. | Future tooling |
| CI/CD or release integration | Pipeline publishes definitions independently from runtime startup. | Future integration |
| Init container, sidecar, or deployment hook | Platform bootstrap owns management credentials and publishes before the app becomes ready. | Future integration |
| Pull reconciler or GitOps | Controller observes desired bundles and reconciles registry state. | Future integration |
| Registry-first | Operator tooling owns definitions; application references an existing binding. | Supported architecture |

These are control-plane clients, not alternate service architectures. None may register through the data-plane decide endpoint.

## MVP startup registration

Intent-level startup:

```ts
const flaggo = await createFlaggoClient({
  controlPlane: {
    mode: "startup-register",
    bundle: generatedDecisionBundle,
    credential: managementCredential
  },
  dataPlane: {
    serviceUrl: "https://flaggo.example.com"
  }
});
```

The exact SDK shape remains provisional, but behavior is fixed:

1. Load the statically extracted canonical bundle; do not derive semantics from whichever runtime branch executes.
2. Call the management validate/apply operation, not the decide endpoint.
3. Use a deterministic idempotency key derived from application, environment, and `bundleDigest` so concurrent replicas submitting identical bundles converge on one result.
4. Accept only an approved registration receipt.
5. Initialize the data-plane binding from the returned definition ID, revision, and digest.
6. Permit decision calls only after registration succeeds.

Startup registration does not bypass lifecycle or approval:

- invalid bundle: startup registration fails with structured issues,
- semantic change requiring approval: `createFlaggoClient` rejects with a typed `requires-approval` error and does not initialize the data-plane binding,
- identical bundle: registry returns the existing immutable identity,
- concurrent identical startup: idempotent replay returns the same receipt,
- conflicting bundle: startup registration fails; no previous revision is selected.

The Flaggo client initialization rejects on validation failure, apply failure, conflict, or `requires-approval`. The host application decides whether to stop startup or continue without Polari, but it cannot turn that failure into a local decision fallback.

The typed approval error includes the stable `approvalRequestId`. Approval atomically applies the pending canonical bundle; a later startup retry or restart with the same bundle receives the stored approved receipt and may initialize the data plane.

### Credential boundary

Startup registration requires control-plane authority. Production clients use OAuth 2.0/OIDC credentials: bundle validation requires `polari.definitions:validate`, apply requires `polari.definitions:apply`, and approval requires `polari.definitions:approve`. Long-lived management credentials must not be embedded in browser bundles or other untrusted clients.

For the local Tetris MVP, a local trusted bootstrap host or explicitly insecure local-development mode may perform registration. A production browser deployment must use another control-plane client experience, such as a backend bootstrap endpoint, init/deployment hook, CLI/CI publication, or registry-first binding.

## Tooling experience

Intent-level commands:

```text
flaggo contracts extract
flaggo contracts validate
flaggo contracts apply
```

SDK extraction and CLI tooling should:

- partition static definition semantics from runtime values,
- normalize and hash the canonical bundle,
- show semantic changes before apply,
- submit bundles to management APIs,
- return structured validation issues,
- expose the accepted definition ID, revision, and digest,
- optionally generate a runtime binding for the application.

For MVP startup mode, the SDK consumes the registration receipt directly as its runtime binding. In other modes, the binding may come from generated code or configuration. It is convenience metadata, not proof of authorization; the data plane always verifies it against registry state.

## Data-plane contract precondition

Every production decide request must identify the exact expected definition using:

```text
definitionId + revision + contractDigest
```

The data plane never:

- registers a definition from a runtime request,
- accepts a full inline definition,
- infers a new revision from the decision key,
- silently selects the latest or previous revision,
- treats contract/configuration failure as a fallback decision.

Known older revisions can continue operating during rolling deployments only when the exact revision remains registered and allowed.

## Runtime error behavior

| Condition | Behavior | SDK local fallback |
| --- | --- | --- |
| Missing expected contract identity | `400 missing-contract-identity` | Forbidden |
| Unknown decision key | `404 unknown-decision-key` | Forbidden |
| Unknown definition ID/revision | `409 contract-not-registered` | Forbidden |
| Digest conflicts with registered definition | `409 contract-conflict` | Forbidden |
| Definition is retired | `409 retired-definition` | Forbidden |
| Invalid context or inference input | `400` or `422` Problem Details | Forbidden |
| Registered definition evaluates but policy/evidence blocks adaptation | `200` audited server fallback | Not applicable |
| Data plane is unavailable, unreachable, or times out | Transport/availability failure | Explicitly configurable |

Contract errors are actionable deployment or control-plane mistakes. Converting them into local values would hide drift and make an unregistered build appear healthy.

## Three distinct outcomes

### Approved server decision

The registered definition is evaluated and produces an approved value or strategy result. The response contains server decision, policy, definition, audit, and exposure-confirmation identity.

### Governed server fallback

The registered definition is valid, but evidence, policy, governed state, or safety prevents adaptation. The server returns the definition's registered fallback as an audited `200` decision result.

### SDK availability fallback

The data plane cannot be reached or cannot complete a request because of an explicitly recognized availability failure. If application configuration enables it, the SDK may return the code-declared local default with `source: "client-fallback"`.

An availability fallback has no server `decisionId`, `auditId`, policy result, definition status, or exposure token. It is never used for a 4xx contract/configuration response.

Availability fallback is disabled by default. Applications must explicitly enable the code-declared local default for recognized conditions such as connection failure, timeout, or `502`/`503`/`504`. If it is disabled, the SDK surfaces an availability error instead.

## SDK error surface

The basic `flaggo.tune.*` promise has three outcomes:

```text
server decision or governed fallback
  -> resolve ServerDecisionReceipt

recognized availability failure + fallback enabled
  -> resolve ClientFallbackReceipt

contract/configuration error, or availability fallback disabled
  -> reject with typed Flaggo error
```

Contract errors expose the server Problem Details payload, including stable `code`, status, correlation ID, and structured issues. SDK consumers branch on those fields, never on error messages.

The SDK must not catch a contract error and return a success-shaped local value. Direct REST clients receive the same Problem Details contract without SDK-specific fallback behavior.

## Failed control-plane apply

Atomic bundle apply means a failed bundle produces no registry mutations. Existing registered definitions continue serving clients that explicitly reference them.

If code expecting a failed or unapplied new definition is deployed:

```text
new build sends new definitionId/revision/digest
  -> data plane cannot resolve exact identity
  -> 409 contract-not-registered
  -> SDK surfaces the contract error
  -> no old revision and no local fallback are selected
```

This preserves runtime correctness across every control-plane client experience.

## Local development

Local-only authoring may evaluate declarations without a remote control plane when explicitly configured. It is a separate development mode, not production data-plane behavior.

Production mode requires registered identity for remote decision calls. Startup registration is permitted only from a trusted application/bootstrap environment with explicit control-plane credentials; it never happens implicitly inside a decide request.

## Design rule

> Control-plane publication may be coordinated by application startup, CLI, automation, or operators; exact registered identity remains a hard data-plane precondition.
