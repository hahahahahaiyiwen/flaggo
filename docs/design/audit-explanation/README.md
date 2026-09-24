# Audit and Explanation Design

## Purpose

Audit records make Flaggo decisions reconstructable. Explanation text makes them understandable to operators and developers.

For the MVP, audit should be local-first and simple, but a ready data plane
returns a successful decision only after its audit record is durably committed.
`FileAuditSink` is the minimum local hero-path implementation. Console and
in-memory records are limited to tests or explicitly non-ready debugging modes;
they cannot authorize a successful decision response.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Phase 1 wire-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md).

## MVP responsibility

Audit records should capture:

- request metadata,
- decision key and definition,
- requested runtime targets, resolved control targets, and policy targets,
- contract version,
- runtime context summary,
- authenticated tenant/application/environment, original caller inputs, resolved
  input values and per-input ownership/binding/generation/source timestamps,
- evidence snapshot summary when evidence participated,
- whether authority was selected and which current authority kind participated,
- required state ID, generation, proposal, activation, and approval lineage for
  every selected authority, plus strategy ID for a numeric rule,
- candidate value,
- policy result,
- returned value,
- fallback usage,
- reason text.

Exposure confirmation is a separate auditable transition. The decision audit records what Flaggo returned; the exposure record identifies that the application confirmed applying or rendering that value. Outcome attribution uses the resulting `exposureId`.

The runtime response should stay compact. The audit record can contain richer evidence and state details.

## Core port

```ts
interface IAuditSink {
  record(input: AuditRecord): Promise<void>;
}
```

The Decision API owns audit identity. It preallocates `auditId`, places the
same value in the server response and `AuditRecord`, and returns the response
only after the sink acknowledges a durable commit. Successful completion of
`IAuditSink.record` in a ready data plane means the record crossed the sink's
durability boundary and survives process failure. A volatile sink must not
report that completion in a ready configuration. A sink never substitutes
another identity.

MVP implementations:

| Implementation | Use |
| --- | --- |
| `ConsoleAuditSink` | Explicitly non-ready local debugging only. |
| `InMemoryAuditSink` | Tests and explicitly non-ready operator-view prototypes only. |
| `FileAuditSink` | Minimum durable local sink for the ready hero path. |

Cloud implementations can later write to object storage, event streams, or managed logging behind the same port.

## Explanation model

The MVP should generate explanation text from structured facts rather than free-form model reasoning.

Example:

```text
Approved strategy slowed the drop interval because board pressure was high and recent placement time was slow.
```

Explanation inputs:

- matching strategy rule reason,
- policy result,
- fallback reason,
- evidence quality when evidence participated,
- scope resolution fallback reason.

## Runtime behavior

```text
Decision API
  -> preallocates decisionId and auditId
  -> builds response and audit record with the same auditId
  -> writes through IAuditSink
  -> returns the response only after durable commit succeeds
```

If audit writing fails, the MVP should fail safe. For local development, surfacing the error is preferable to silently returning unaudited decisions.

The proposed Phase 1 runtime wire contract guarantees `auditId` only for server-produced decision results. SDK-local availability fallback has no server decision or audit identity. Audit query endpoints remain outside the required Phase 1 executable artifact set; local sinks and direct inspection are sufficient for the first integration slice.

The exposure confirm token is an authorization capability, not audit data. The Decision API must project the server response into `AuditDecisionResult` and remove `confirmToken` before calling any audit sink, including console and file sinks.

## Tetris MVP audit example

The expanded state-summary example below is the accepted final Phase 3 audit
target, not the current persisted envelope. #41 owns that remaining
authority-lineage re-baseline. Current `DecisionAuditRecord` persists
tenant/app/environment, exact contract, targets, policy/fallback, reason,
`inputs` (resolved values), `requestInputs`, and `inputProvenance`, including
binding/generation/source nanoseconds and observed-only correlation when used.
It never copies unrelated raw telemetry or a confirmation capability.

```json
{
  "auditId": "audit-789",
  "timestamp": "2026-07-29T19:20:00Z",
  "decisionKey": "tetris.dropInterval",
  "request": {
    "decisionKey": "tetris.dropInterval",
    "runtimeTarget": {
      "type": "session",
      "id": "game-456"
    },
    "runtimeContext": {
      "sessionId": "game-456",
      "userId": "user-123",
      "cohort": "new_players"
    },
    "inputs": {
      "boardPressure": 0.82,
      "currentLevel": 3,
      "recentPlacementTimeMs": 1420,
      "recoveryFailures": 2
    },
    "expectedContract": {
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "contractDigest": "sha256:contract...",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
    },
    "client": {
      "appId": "tetris-demo",
      "environment": "dev"
    }
  },
  "response": {
    "decisionKey": "tetris.dropInterval",
    "definition": {
      "appId": "tetris-demo",
      "environment": "dev",
      "key": "tetris.dropInterval",
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3"
    },
    "decisionId": "decision-123",
    "value": 850,
    "valueType": "number",
    "decisionMode": "strategy",
    "strategyId": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
    "runtimeTarget": {
      "type": "session",
      "id": "game-456"
    },
    "controlTarget": {
      "type": "cohort",
      "id": "new_players"
    },
    "targetProvenance": [
      {
        "targetType": "cohort",
        "claimedId": "new_players",
        "resolvedId": "new_players",
        "source": "client-verified"
      }
    ],
    "resolutionChain": [
      "session:game-456",
      "cohort:new_players",
      "global"
    ],
    "confidence": null,
    "fallback": {
      "source": "server",
      "resolutionFallbackUsed": true,
      "decisionFallbackUsed": false,
      "reason": "no_active_session_authority"
    },
    "policy": {
      "result": "approved",
      "reasons": [],
      "appliedConstraints": ["number-bounds", "step", "max-delta"]
    },
    "definitionStatus": {
      "definitionId": "def_01JQ8Y7M6X3K9P2W4R5T6V7N8A",
      "revision": "rev_01JQ8YB4E5H6J7K8M9N0P1Q2R3",
      "contractDigest": "sha256:contract...",
      "bundleDigest": "sha256:bundle...",
      "buildId": "tetris-web-2026-07-25.1",
      "deploymentId": "tetris-web-dev-a",
      "integrity": "verified",
      "compatibility": "identical"
    },
    "exposure": {
      "confirmationRequired": true
    },
    "reason": "The approved weighted numeric rule met its 0.55 threshold.",
    "auditId": "audit-789"
  },
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "stateSummary": {
    "authoritySelected": true,
    "authorityKind": "numeric-rule",
    "strategyId": "strategy_01JQ8YJ6K7L8M9N0P1Q2R3S4T5",
    "stateId": "state_01...",
    "generation": 1,
    "proposalId": "proposal_01...",
    "activationId": "activation_01...",
    "approvalReference": "approval_01..."
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "step", "max-delta"]
  },
  "reason": "The approved weighted numeric rule met its 0.55 threshold."
}
```

The top-level and response `auditId` values are intentionally identical. The
audit response projection omits the exposure confirmation token while
retaining whether confirmation was required.

No-compatible-authority fallback instead records
`{ "authoritySelected": false, "resolution": "server-fallback" }`. When policy
rejects a candidate after authority selection, the response records fallback
but the audit preserves that selected authority's complete lineage.

## MVP non-goals

- Full audit query API.
- Long-term retention policy.
- LLM-generated explanations.
- Redaction pipeline beyond avoiding sensitive raw telemetry in runtime responses.
