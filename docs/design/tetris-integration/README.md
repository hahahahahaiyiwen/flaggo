# Phase 3 Tetris Integration

## Goal

Prove `tetris.dropInterval` end to end with the real control plane, data plane,
TypeScript SDK, exposure flow, local telemetry, and inspectable audit output.
The integration remains cloud-free, obtains authenticated approval for the
bundle-declared initial authority, and does not place management credentials or
trusted registration behavior in browser code.

## Scope and ownership

- `examples/tetris-integration` owns the canonical Phase 3 bundle, trusted
  bootstrap command, local harness, and audit/telemetry inspection workflow.
- The bundle owns the `bundle-approved` initial authority candidate, including
  the `cohort:new_players` target, weighted numeric rule, and rationale. It
  does not own trusted proposal, activation, state, or approval identities.
- The control plane validates the exact candidate, records authenticated
  approval, derives proposal and activation identities, and publishes state
  through expected-baseline compare-and-swap.
- The state module owns durable activation and read-only runtime projection.
  The data-plane host selects it through configuration and does not encode
  Tetris strategy behavior in `Program.cs`.
- The reasoning module executes an explicit weighted numeric rule. The rule
  requires `boardPressure`, `recentPlacementTimeMs`, `recoveryFailures`, and
  `currentLevel`; application code does not reproduce its decision logic.
- The audit module owns local decision and confirmed-exposure records. Local
  inspection uses an explicit file/tool boundary rather than production
  runtime debug routes. Existing JSON Lines records must pass strict readiness
  parsing, and exposure audit append precedes confirmation commit.
- Outcome linkage uses the existing SDK `TelemetrySink`: after exposure
  confirmation, the caller emits `tetris.outcomeObserved` with the returned
  `decisionId` and `exposureId`. Phase 3 adds no new runtime wire operation.

## Local lifecycle

1. Start the control plane against an isolated registry path.
2. Run the trusted bootstrap command with the canonical bundle.
3. If the SDK returns typed `RequiresApprovalError`, bootstrap approves the
   immutable snapshot through the management API and retries registration.
4. The service derives and durably activates the approved rule. Registration
   remains non-ready until the required authority is active.
5. Start the data plane against the same registry, state root, and audit path.
   Hosting resolves active state through the state module.
6. Drive SDK and direct REST decisions, confirm only applied receipts, emit
   linked outcome telemetry, and inspect the local JSON Lines records.

## Acceptance criteria

- SDK and direct REST serialization produce contract-equivalent decisions.
- Code-first extraction and the canonical bundle fixture produce byte-identical
  normalized Tetris definitions and the same `contractDigest`; output range
  validation is not duplicated as a synthesized policy constraint.
- The bundle is rejected if the initial rule references undeclared inference
  inputs, references a nonnumeric or non-app-emitted metric, or violates
  target, action-space, fallback, or policy constraints.
- The completed registration receipt identifies the proposal, activation,
  server-derived strategy, active state, generation, control target, and
  numeric-rule kind.
- Exact startup retries return the same activation and state. A stale baseline
  cannot overwrite newer authority.
- Semantic-revision replacement compare-and-swaps the same stable
  application/environment/decision-key/control-target authority head.
- Activation derives one opaque strategy ID from the activation and canonical
  rule; the ready receipt, state, runtime response, and audit use that same ID,
  including exact replay.
- High pressure plus slow placement selects `850ms`, exactly one allowed
  `50ms` delta above the `800ms` contract baseline.
- The real host computes all four normalizations and weights. A `0.9`
  board-pressure vector with the other inputs at minimum scores `0.405` and
  selects `750ms`; a `0.4` board-pressure vector with the other inputs at
  maximum scores `0.73` and selects `850ms`. Both oppose the
  board-pressure-only threshold result.
- With board pressure fixed at `0.5`, paired vectors independently move
  placement time, recovery failures, and current level across the `0.55`
  aggregate threshold. The harness asserts response value, strategy ID,
  reason, and persisted audit inputs for every vector.
- Recovery selects `750ms`, exactly one allowed delta below the baseline.
- A ready definition with no compatible active state at any permitted target,
  or a runtime-policy block, returns the audited server fallback of `800ms`.
- Pending activation, failed readiness, corrupt persistence, and incoherent
  state fail initialization/readiness rather than returning `800ms`.
  `definition-not-ready`, `decision-service-not-ready`, and
  `invalid-decision-state` are explicitly ineligible for SDK fallback.
- Data-plane unavailability produces an SDK-local `800ms` fallback whose
  provenance remains distinct from server-policy fallback.
- Applying and confirming an approved authority decision creates a
  confirmed-exposure audit record and permits linked outcome telemetry; an
  unused receipt or server fallback creates neither.
- Audit output records strategy ID, all four inputs, policy result, fallback
  provenance, and confirmation linkage.
- Bundle-authored authority records authored rationale and approval provenance,
  but does not claim learned evidence quality, model uncertainty, expected
  outcome, or sample size.
- Invalid persisted modes, incoherent strategy fields, nonnumeric strategy
  values, both/neither authority payloads, discriminator mismatches, and
  nonfinite numeric-rule parameters make state readiness fail.
- Malformed or torn audit files fail readiness. Audit append failure leaves a
  receipt unconfirmed, while replay after recovery reuses one exposure record.
- Failed activation publication is atomic: readers see either the complete old
  state or the complete new state.
- Any control/data host exit before requested shutdown fails the harness even
  when the remaining workflow assertions would otherwise pass.
- No browser-facing code receives management credentials, and no production
  runtime route exposes local audit or registry internals.

## Maintenance

Keep the bundle, bootstrap, activation boundary, and harness assertions
cohesive. Contract-breaking changes must update OpenAPI, schemas, fixtures, and
both conformance suites before this integration. Internal adapter or strategy
changes require behavior-seam tests and corresponding module README updates.
