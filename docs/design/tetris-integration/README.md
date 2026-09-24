# Tetris integration

## Goal and ownership

Prove `tetris.dropInterval` with the real control/data-plane hosts, generated
TypeScript client, explicit confirmation, application-owned OTel logs and
inspectable durable records. The harness is cloud-free and keeps management
credentials outside browser/runtime code.

Logical ownership follows Contract Service, Decision Service, State Store,
Evidence Store and OTel Ingestion. Current assemblies are implementation
libraries, not standalone Policy, Audit, Reasoning or Operator Console services.

`examples/tetris-integration/tetris-definition-bundle.json` is the sole
authored manifest. Compilation generates its normalized bundle and typed
catalog. Four request inputs and result/context/target/constraint semantics
belong to the manifest; call sites contain only keys and live data.

Current trusted tooling publishes the manifest, handles `RequiresApprovalError`
with explicit exact-snapshot local approval, reapplies, and provisions
receipt-bound existing state. Runtime initializes synchronously without
registration. This local state bootstrap is not another public definition
format or an activation-ready receipt.

#49 aligns executable server boundaries; #40 adds manifest initial authority
and activation-converged registration; #41 owns the final bundle-approved
Tetris run. Their outcomes are not claimed by the current harness.

## Current local flow

1. Start isolated Contract Service publication and explicitly apply/approve.
2. Publish one digest-pinned generation of receipt, local state and empty
   quality evidence, then start the data plane against it.
3. Initialize the runtime client from generated catalog and approved receipt.
4. Drive SDK and direct REST calls with plain live input maps.
5. Apply a result, confirm it, and attach confirmed attributes to an ordinary
   `game.outcome` OTel log using the application's logger/provider.
6. Inspect decision, exposure and telemetry files through local tools.

No exporter is initialized by the Flaggo client. The
[separate Collector example](../../../examples/otel-evidence/README.md)
proves materialized evidence operands and attributed outcome bindings without
substituting delayed telemetry for current game state.

## Implemented acceptance boundaries

- Generated compiler/catalog artifacts stay fresh; SDK and REST share exact
  identities and plain-input semantics.
- High pressure/slow placement returns exactly `850ms`; recovery returns
  `750ms`, independently within delta `50` of default `800ms`.
- All four normalizations matter: pressure `0.9` with other inputs at minimum
  scores `0.405` and selects `750ms`; pressure `0.4` with others at maximum
  scores `0.73` and selects `850ms`. Paired vectors independently move placement
  time, failures and level across threshold `0.55`.
- Rules return null learned confidence. Direct-live-input calls depend on
  neither materialization nor an empty quality fixture.
- The existing last-change cooldown fixture returns a recorded `800ms` server
  fallback. A separate outage proves explicitly enabled SDK `800ms` fallback
  without fabricated server IDs or exposure.
- Records retain caller/resolved values, input provenance, constraint result,
  reason and target/strategy identity. Confirmation copies immutable
  decision-time facts. Exactly one explicitly applied/confirmed result creates
  exposure and correlated native outcome telemetry; unused receipts do not.
- Append failure cannot create a newly confirmed exposure. Malformed/torn
  persistence is not repaired into success.
- Bootstrap publication is atomic, cleanup is ownership-scoped and unexpected
  host exit fails the harness.

## Final Phase 3 extension

The later integrated path must also prove:

1. Contract Service validates the manifest's initial rule/targets/constraints,
   approves the exact snapshot, activates via State Store CAS and withholds
   readiness until required authority is active.
2. Publication replay preserves approval, activation, state, generation and
   derived strategy identities; a stale baseline cannot replace newer authority.
3. State and record failures remain explicit, and publication exposes either
   the complete previous or replacement generation.
4. Evidence Store records complete state/activation lineage and confirmed
   outcomes under the aligned contracts, without standalone Policy/Audit
   service dependencies.
5. The real Tetris application uses the canonical service/store boundaries,
   trusted publisher, key-based SDK and its existing OTel pipeline.

Those #49/#40/#41 obligations retain the guarantees in
[Authority](../../architecture/AUTHORITY.md). No dormant initial-authority
flag or compatibility layer is added to claim their completion.

Keep this document, [run instructions](../../../examples/tetris-integration/README.md),
generated artifacts, harness assertions and owning module contracts aligned.
