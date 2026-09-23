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
  authorized actor approves the exact canonical bundle when required
  authority-workflow branch:
    proposal-managed -> registry publishes the definition and returns a ready receipt
    bundle-approved -> service activates state through expected-baseline
      compare-and-swap and returns a ready receipt with authority identities
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
4. Accept only a ready registration receipt: immediately after approved
   proposal-managed publication when no initial authority exists, or after all
   required bundle-approved authority is active.
5. Verify that the receipt includes definition identity and, for
   bundle-approved definitions, proposal, activation, state, generation,
   target, and strategy-kind references.
6. Initialize the data-plane binding from the returned definition ID,
   revision, and digest.
7. Permit decision calls only after registration succeeds.

Startup registration does not bypass lifecycle or approval:

- invalid bundle: startup registration fails with structured issues,
- semantic change requiring approval: `createFlaggoClient` rejects with a typed `requires-approval` error and does not initialize the data-plane binding,
- identical bundle: registry returns the existing immutable identity,
- concurrent identical startup: idempotent replay returns the same receipt,
- conflicting bundle: startup registration fails; no previous revision is selected.

The Flaggo client initialization rejects on validation failure, apply failure, conflict, or `requires-approval`. The host application decides whether to stop startup or continue without Polari, but it cannot turn that failure into a local decision fallback.

The typed approval error includes the stable `approvalRequestId`. Approval
authorizes the exact pending canonical bundle snapshot. For a proposal-managed
definition, approval publishes the definition and stores a ready receipt
without an activation plan or authority references. For bundle-approved
authority, approval derives proposal and activation identities, stores the
captured-baseline activation plan, and publishes state through
expected-baseline compare-and-swap. A later startup retry or restart with the
same bundle receives the stored ready receipt and may initialize the data
plane.

If that approval expires before authorization, the next startup apply uses the
same deterministic key but triggers server-side revalidation and receives one
fresh linked approval request. Concurrent replicas converge on the replacement
request; the SDK does not need a renewal endpoint or a new locally generated
key. Once approved publication is ready, exact retries return the same receipt.
For bundle-approved authority they also return the same activation and state
instead of creating new authority.

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

Known older revisions can continue issuing requests during rolling deployments
only when the exact revision remains registered and allowed. If a newer
revision has replaced the stable authority head, the older request receives
server fallback unless another permitted target has exact compatible state; it
never consumes the newer strategy.

## Runtime error behavior

| Condition | Behavior | SDK local fallback |
| --- | --- | --- |
| Missing expected contract identity | `400 missing-contract-identity` | Forbidden |
| Unknown decision key | `404 unknown-decision-key` | Forbidden |
| Unknown definition ID/revision | `409 contract-not-registered` | Forbidden |
| Digest conflicts with registered definition | `409 contract-conflict` | Forbidden |
| Definition is retired | `409 retired-definition` | Forbidden |
| Required activation is pending or failed | `409 definition-not-ready` | Forbidden |
| Invalid context or inference input | `400` or `422` Problem Details | Forbidden |
| A required state, policy, or audit readiness check failed | `503 decision-service-not-ready` with `clientFallback.eligible: false` | Forbidden |
| Persisted state violates canonical invariants | `500 invalid-decision-state` with `clientFallback.eligible: false` | Forbidden |
| Registered definition evaluates but applicable state, policy, or evidence blocks adaptation | `200` audited server fallback | Not applicable |
| Required evidence is unavailable and governed fallback is forbidden | `503 required-evidence-unavailable` with `clientFallback.eligible: false` | Forbidden |
| Data plane is genuinely unavailable, unreachable, or times out after readiness passed | Transport failure or `503 service-unavailable` with `clientFallback.eligible: true` | Explicitly configurable |

Contract and readiness errors are actionable deployment, control-plane, or
operator failures. Converting them into local values would hide drift or
corruption and make an unhealthy deployment appear healthy.

## Three distinct outcomes

### Approved server decision

The registered definition is evaluated and produces an approved value or strategy result. The response contains server decision, policy, definition, audit, and exposure-confirmation identity.

### Governed server fallback

The registered definition is valid, but governed state, policy, safety, or
explicitly required evidence prevents adaptation. The server returns the
definition's registered fallback as an audited `200` decision result.

### SDK availability fallback

The data plane cannot be reached or cannot complete a request because of an explicitly recognized availability failure. If application configuration enables it, the SDK may return the code-declared local default with `source: "client-fallback"`.

An availability fallback has no server `decisionId`, `auditId`, policy result, definition status, or exposure token. It carries only the accepted expected contract tuple as client provenance.

Availability fallback is disabled by default. It is eligible only after configured retries for DNS/connection failure, connection/read timeout before a complete response, intermediary `502`/`504`, or a valid Flaggo `5xx` Problem Details response with `clientFallback.eligible: true`. It is forbidden for TLS, certificate, proxy/authentication configuration, caller cancellation, malformed responses, every `503` without explicit eligibility, every `4xx`, `500`/`501`/`505`, and Flaggo problems where eligibility is false or absent.

`required-evidence-unavailable` is always ineligible for SDK-local fallback.
When governed fallback is permitted, the server returns an audited fallback
decision. Otherwise the SDK surfaces fallback-ineligible Problem Details.

The default is one retry after the initial attempt with the same decide
idempotency key. Before any remote attempt or local fallback, the generated
call-site digest must match
`acceptedDefinitions[decisionKey].contractDigest` from the ready registration
receipt. Missing or mismatched binding is a local contract error, not
availability.

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

## Failed or pending control-plane apply

An apply that fails or remains pending produces no accepted runtime binding
for the requested new or changed definition. Existing registered definitions
continue serving only callers that explicitly reference their complete accepted
`{ definitionId, revision, contractDigest }` tuple.

Until the requested binding is accepted, client data-plane calls remain
disabled locally. A direct request that bypasses this client precondition may
receive `409 contract-not-registered`; it must not fabricate a new identity,
select an older revision, or use local fallback.

This preserves runtime correctness across every control-plane client experience.

## Local development

Local-only authoring may evaluate declarations without a remote control plane when explicitly configured. It is a separate development mode, not production data-plane behavior.

Production mode requires registered identity for remote decision calls. Startup registration is permitted only from a trusted application/bootstrap environment with explicit control-plane credentials; it never happens implicitly inside a decide request.

## Design rule

> Control-plane publication may be coordinated by application startup, CLI, automation, or operators; exact registered identity remains a hard data-plane precondition.
