# Control Plane

The control-plane host exposes definition validation, bundle application, and
approval lifecycle operations.

It composes the registry-owned definition lifecycle ports. It never serves
runtime decisions and never registers definitions as a side effect of a
data-plane request. Keep its wire behavior aligned with
`contracts/openapi/flaggo-management-v1.yaml`.

## Current implementation

`src/Flaggo.ControlPlane` is an executable .NET 10 composition root exposing
definition validate/apply plus approval status, immutable snapshot,
approve, and reject operations. It uses the local registry adapter and preserves
the frozen strict JSON, Problem Details, correlation, digest-header,
idempotency, and approval semantics.
It also registers registry-owned runtime and intelligence read ports for
lifecycle composition. The intelligence port returns typed
objectives, signal roles, workflow permissions, action space, and safety
envelope; control-plane consumers do not parse approval snapshots or registry
storage.

Management endpoints require OAuth bearer authentication and their
operation-specific definition scopes. The explicit
`Flaggo__Authentication__LocalDevelopmentBypass=true` setting is accepted only
in the Development environment and supplies definition-management and
`polari.lifecycle:review` / `polari.lifecycle:activate` scopes, not runtime
decision or exposure scopes.

Run locally:

```powershell
dotnet run --project apps\control-plane\src\Flaggo.ControlPlane
```

The default adapter persists to the repository-local
`.flaggo/definition-registry-v1.json`, shared with the data-plane host. Override
it with an absolute `Flaggo__Registry__LocalFilePath` value when required.
Apply idempotency, pending snapshots, approval baselines, expiry, and terminal
decisions survive host restart and are committed under a cross-process lock.
Storage errors are surfaced; the host never substitutes a separate in-memory
registry.

For the Phase 3 Tetris flow, trusted registration remains an external startup
operation under `examples/tetris-integration`. The command consumes the
canonical bundle, handles typed approval, and retries registration. The
control-plane host remains a generic management composition root and never
embeds Tetris bootstrap behavior or exposes management credentials to browser
code.

## Internal lifecycle composition

`LocalLifecycleHosting` registers `IProposalGovernance` without adding routes
to the frozen management API. The current path performs review, explicit
automatic approval, and separate audited activation. Human-required policy
stays `pending-approval`; manual approval workflow/UI is not implemented.
Issue #25 supplies the operator/scripted producer entry point.

`HttpLifecycleActorProvider` uses the authenticated identity's subject claim,
issuer, exact application/environment claims, and lifecycle operation scopes.
Definition-approval permission alone grants no proposal authority. Producer
source labels and supplementary anonymous claims cannot grant permissions.
Non-HTTP trusted hosts must supply their own `ILifecycleActorProvider`.

Before resolving governance, configure `Flaggo:State:LocalFilePath` as a
committed-artifact descriptor for one application/environment. Configure the
same direct state descriptor in the data plane, with
`Flaggo:Bootstrap:LocalGenerationPath` unset. Governance rejects bootstrap
generation composition rather than writing authority behind a runtime still
pinned to a separate bootstrap artifact. It never substitutes an in-memory
writer for missing configuration or file failures.

Example host configuration:

```json
{
  "Flaggo": {
    "State": { "LocalFilePath": "C:\\flaggo-local\\state.commit.json" },
    "Lifecycle": {
      "CommitTimeout": "00:00:05",
      "EvidencePath": "C:\\flaggo-local\\proposal-evidence.commit.json",
      "Policies": [
        {
          "AppId": "tetris-demo",
          "Environment": "dev",
          "DecisionKey": "tetris.dropInterval",
          "EnvironmentPolicy": {
            "Revision": "environment-v1",
            "AllowedTargetKinds": ["cohort"],
            "AllowedTargets": [{ "Type": "cohort", "Id": "new_players" }],
            "AutomaticApprovalAllowed": true,
            "AllowSupersession": true
          },
          "OperatorControls": {
            "Revision": "operator-v1",
            "AllowedTargetKinds": ["cohort"],
            "AutomaticApprovalAllowed": true,
            "AllowSupersession": true,
            "MaximumEvidenceAgeSeconds": 300
          }
        }
      ]
    }
  }
}
```

Policy contexts are reread for each new review/activation, including updated
operator controls. Unknown settings or malformed layers fail explicitly.
Missing policy holds; automatic approval and replacement permissions default
to false. Layer permissions intersect with registered definition constraints.

`EvidencePath` is a committed proposal-evidence catalog, not raw JSON or the
runtime strategy-ID catalog. An unconfigured catalog provides no evidence;
required references then hold. A configured but unavailable catalog raises an
explicit dependency error and never falls back to an empty catalog.
Trusted publishers use `CommittedFileSnapshotWriter` with
`LifecycleJson.Bytes(new ProposalEvidenceDocument(1, snapshots))`.

`IGovernedStateLifecycleStore` and `ILifecycleAuditReader` share the same atomic
file adapter. A five-second default commit wait is linked to request
cancellation and host shutdown. An interrupted wait may have committed;
retry the original operation identity to reconcile its immutable receipt.
