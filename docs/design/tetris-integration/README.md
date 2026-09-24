# Phase 3 Tetris integration

## Goal

Prove `tetris.dropInterval` end to end through Contract Service, Decision
Service, State Store, Evidence Store, OTel Ingestion, a trusted manifest
publisher, and the key-based runtime SDK.

The integration remains cloud-free and does not place management credentials,
static definitions, or trusted registration behavior in browser/runtime code.

## Ownership

- `examples/tetris-integration` owns the canonical hand-authored manifest,
  trusted publication command, local harness, and store inspection workflow.
- Contract Service validates the manifest, records authenticated approval, and
  orchestrates State Store activation.
- State Store owns the stable authority head, immutable state, replay, CAS, and
  lineage.
- Decision Service executes the approved weighted rule, evaluates definition
  constraints, appends a DecisionRecord, and confirms exposures.
- OTel Ingestion validates exposure-linked outcome attribution. Evidence Store
  owns the resulting observations, DecisionRecords, exposure records, and
  Outcome records.
- The application owns its OTel provider/exporter and sends data to OTel
  Ingestion.

## Local lifecycle

1. Start Contract Service against isolated Contract and State Stores.
2. Publish the canonical manifest through trusted deployment/control-plane
   tooling.
3. Approve the exact immutable snapshot when required.
4. Contract Service activates the initial numeric rule through expected-head
   compare-and-swap and returns a ready binding.
5. Start Decision Service against the same stores and durable Evidence Store.
6. Use the runtime SDK or direct REST with the decision key plus live target,
   context, and input values.
7. Confirm only applied results, emit exposure-linked outcomes through the
   application's OTel pipeline, and inspect durable store records.

## Acceptance criteria

- The hand-authored manifest is the sole static definition source.
- Runtime call sites contain only the decision key and live target/context/input
  values; no inline definition, signal handle, AST extraction, or management
  credential is present.
- SDK and direct REST requests produce contract-equivalent decisions.
- The manifest is rejected when its rule references undeclared or invalid
  inputs, violates target/action/fallback semantics, or conflicts with
  definition constraints.
- Registration remains non-ready until approved authority is active.
- Exact publication retry returns the same activation, state, generation, and
  numeric-rule strategy identity.
- A stale baseline cannot overwrite newer authority.
- The real host normalizes and weights all four inputs:
  - a `0.9` pressure vector with other inputs at minimum scores `0.405` and
    selects `750ms`;
  - a `0.4` pressure vector with other inputs at maximum scores `0.73` and
    selects `850ms`.
- Paired vectors independently move placement time, recovery failures, and
  level across the `0.55` threshold.
- High pressure plus slow placement returns exact approved `850ms`; recovery
  returns exact approved `750ms`.
- Both branches independently satisfy fixed-default `max-delta = 50` against
  `800ms`.
- No compatible authority or a constraint-required fallback returns the
  governed `800ms` with explicit server provenance.
- Decision Service unavailability may produce configured SDK-local `800ms`
  with no server decision, record, or exposure identity.
- Pending activation, corrupt stores, invalid state, and non-ready durable
  append fail readiness rather than returning a value.
- Before server success, Evidence Store contains a DecisionRecord with exact
  identity, state/activation lineage, all four inputs, constraint result,
  fallback provenance, and returned value.
- Applying and confirming a result creates one exposure record; an unused
  result creates none.
- Outcome telemetry references the confirmed `exposureId`; ordinary telemetry
  remains raw and unlinked.
- Bundle-authored authority records rationale and approval provenance but no
  learned confidence or evidence claim.
- Malformed or torn Evidence Store files fail readiness. Append failure cannot
  produce a successful response or committed exposure.
- Failed activation publication is atomic: readers see the complete previous or
  replacement state.
- No production route exposes local Contract, State, or Evidence Store
  internals.
- The harness contains no standalone Policy, Audit, Reasoning, or Operator
  Console service dependency.

## Maintenance

Contract-breaking changes update manifest schema, OpenAPI, fixtures, SDK,
services, and conformance together. Internal adapter changes require
behavior-boundary tests and corresponding module documentation.
