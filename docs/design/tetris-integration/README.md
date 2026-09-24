# Tetris integration

## Current goal and ownership

Prove `tetris.dropInterval` with the real control plane, data plane, generated
TypeScript client, explicit exposure flow, native application OTel logs and
inspectable local audit. The harness is cloud-free and browser code never
receives management credentials.

`examples/tetris-integration/tetris-definition-bundle.json` is the sole authored
manifest. Compilation produces its normalized bundle and typed catalog. The
manifest owns four request inputs and result/context/target/policy semantics;
the game never declares Flaggo telemetry producers or repeats the contract at
call sites.

Trusted bootstrap explicitly publishes the manifest, handles
`RequiresApprovalError` by approving the exact snapshot in local tooling,
reapplies, and provisions receipt-bound existing governed state. The runtime
client initializes from catalog plus approved receipt and performs no
registration. Local state provisioning is not a public authoring format.

#40 owns the remaining initial-authority manifest/activation-ready receipt
extension, and #41 owns the final bundle-approved Tetris migration. Their
completion is not claimed by this harness.

## Local flow

1. Start an isolated control plane and explicitly publish/approve the manifest.
2. Publish one digest-pinned generation of receipt, local state and empty
   policy-quality evidence; start the data plane against that generation.
3. Initialize the client synchronously from generated catalog and approved receipt.
4. Drive SDK and direct REST calls with plain live input maps.
5. Apply a returned value, explicitly confirm it, then attach confirmed
   attributes to an ordinary `game.outcome` OTel log.
6. Inspect decisions, exposure and telemetry through local files/tools.

No exporter is initialized by the Flaggo client. The example owns its OTel
logger/provider. The [separate Collector example](../../../examples/otel-evidence/README.md)
proves evidence-owned inputs without delaying current game-state operands.

## Implemented acceptance boundaries

- Compiler/catalog artifacts remain fresh and SDK/REST calls share the same
  manifest semantics, exact identities and plain-input contract.
- High pressure/slow placement returns exactly `850ms`; recovery returns
  `750ms`, both one allowed `50ms` delta from default `800ms`.
- All four weighted normalizations participate. Board pressure `0.9` with
  other inputs at minimum selects `750ms`, while pressure `0.4` with the
  others at maximum selects `850ms`. Paired vectors at fixed pressure move
  placement time, failures and level across the threshold independently.
- Deterministic rules have null learned confidence. Request-input-only calls
  remain independent of input materialization and empty policy evidence.
- The existing cooldown path returns an audited `800ms` server fallback.
  A separate data-plane outage proves explicitly enabled SDK-local `800ms`
  fallback with no fabricated server IDs or exposure.
- Decision audit retains resolved caller values, input provenance, policy,
  reason and target/strategy identity. Confirmation copies decision-time data.
  Exactly one explicitly applied/confirmed result produces an exposure and a
  correlated native outcome; unused receipts do not.
- Failed audit cannot create a newly confirmed exposure. Malformed/torn
  persistence is not repaired or converted to a successful value.
- Bootstrap publication is atomic and cleanup is ownership-scoped. An
  unexpected control/data host exit fails the harness.

## Remaining Phase 3 extension

#40/#41 will connect manifest initial authority to the existing activation
core, return activation-converged receipts with server-derived authority
identities, and verify CAS/replay/reauthorization and complete authority audit
lineage in the final Tetris path. Their accepted guarantees remain in
[Authority](../../architecture/AUTHORITY.md); no dormant manifest flag or
compatibility layer is added here.

Keep this design, the [run instructions](../../../examples/tetris-integration/README.md),
generated artifacts, harness assertions and owning module documentation aligned.
