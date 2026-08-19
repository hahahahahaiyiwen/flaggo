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
It also registers the registry-owned `IIntelligenceDefinitionReader` for
future intelligence and lifecycle composition. That port returns typed
objectives, signal roles, workflow permissions, action space, and safety
envelope; control-plane consumers do not parse approval snapshots or registry
storage.

Management endpoints require OAuth bearer authentication and their
operation-specific definition scopes. The explicit
`Flaggo__Authentication__LocalDevelopmentBypass=true` setting is accepted only
in the Development environment and supplies only management scopes.

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
