# Phase 3 Tetris Integration

## Goal

Prove `tetris.dropInterval` end to end with the real control plane, data plane,
TypeScript SDK, exposure flow, local telemetry, and inspectable audit output.
The integration remains cloud-free and does not place management credentials
or trusted registration behavior in browser code.

## Scope and ownership

- `examples/tetris-integration` owns the canonical Phase 3 bundle, trusted
  bootstrap command, governed-state activation template, local harness, and
  audit/telemetry inspection workflow.
- The registry remains the source of accepted definition identity. Bootstrap
  writes activated state only from the approved registration receipt and
  publishes receipt, state, and evidence through one generation pointer.
- The state module owns the file-backed governed-state adapter. The data-plane
  host selects it through configuration and does not encode Tetris strategy
  behavior in `Program.cs`.
- The evidence module owns the file-backed deterministic local evidence
  adapter that supplies the compact confidence required for strategy results.
  File failure crosses an explicit evidence-unavailable boundary and is
  evaluated by the definition's required-evidence fallback policy.
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
4. Bootstrap durably writes receipt, activated state, and evidence into one
   unique generation, then atomically switches `current.json`.
5. Start the data plane against the same registry, bootstrap generation root,
   and audit path. Hosting resolves state and evidence from one pointer read.
6. Drive SDK and direct REST decisions, confirm only applied receipts, emit
   linked outcome telemetry, and inspect the local JSON Lines records.

## Acceptance criteria

- SDK and direct REST serialization produce contract-equivalent decisions.
- High pressure plus slow placement selects `850ms`, exactly one allowed
  `50ms` delta above the `800ms` governed baseline.
- Recovery selects `750ms`, exactly one allowed delta below the baseline.
- A recent state transition activates cooldown, blocks the candidate, and
  returns the server-policy fallback of `800ms`.
- Data-plane unavailability produces an SDK-local `800ms` fallback whose
  provenance remains distinct from server-policy fallback.
- The canonical strategy requires evidence quality of at least `0.7` and
  forbids client fallback when its active evidence entry is missing; the SDK
  surfaces `required-evidence-unavailable`.
- Applying and confirming a decision creates a confirmed-exposure audit record
  and permits linked outcome telemetry; an unused receipt creates neither.
- Audit output records strategy ID, all four inputs, policy result, fallback
  provenance, and confirmation linkage.
- Invalid persisted modes, incoherent strategy fields, nonnumeric strategy
  values, and nonfinite numeric-rule parameters make state readiness fail.
- Malformed or torn audit files fail readiness. Audit append failure leaves a
  receipt unconfirmed, while replay after recovery reuses one exposure record.
- Failed bootstrap sibling writes are drained and cleaned before publication;
  readers see either the complete old generation or the complete new one.
- Any control/data host exit before requested shutdown fails the harness even
  when the remaining workflow assertions would otherwise pass.
- No browser-facing code receives management credentials, and no production
  runtime route exposes local audit or registry internals.

## Maintenance

Keep the bundle, activation template, bootstrap, and harness assertions
cohesive. Contract-breaking changes must update OpenAPI, schemas, fixtures, and
both conformance suites before this integration. Internal adapter or strategy
changes require behavior-seam tests and corresponding module README updates.
