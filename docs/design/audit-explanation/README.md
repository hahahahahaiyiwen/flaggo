# Audit and Explanation Design

## Purpose

Audit records make Flaggo decisions reconstructable. Explanation text makes them understandable to operators and developers.

For the MVP, audit should be local-first and simple: console, file, or in-memory records are enough if every decision response receives an `auditId`.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Phase 1 wire-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md).

## MVP responsibility

Audit records should capture:

- request metadata,
- decision key and definition,
- requested runtime targets, resolved control targets, and policy targets,
- contract version,
- runtime context summary,
- evidence snapshot summary when evidence participated,
- active value or active strategy,
- state ID, generation, predecessor, proposal, activation, and approval references,
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
  record(input: AuditRecord): Promise<{ auditId: string }>;
}
```

MVP implementations:

| Implementation | Use |
| --- | --- |
| `ConsoleAuditSink` | Local debugging and first demo. |
| `InMemoryAuditSink` | Tests and operator-view prototypes. |
| `FileAuditSink` | Local reproducible audit trail. |

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
  -> builds audit record from request, state, optional evidence, candidate, policy, response
  -> writes through IAuditSink
  -> includes auditId in DecideResponse
```

If audit writing fails, the MVP should fail safe. For local development, surfacing the error is preferable to silently returning unaudited decisions.

The proposed Phase 1 runtime wire contract guarantees `auditId` only for server-produced decision results. SDK-local availability fallback has no server decision or audit identity. Audit query endpoints remain outside the required Phase 1 executable artifact set; local sinks and direct inspection are sufficient for the first integration slice.

The exposure confirm token is an authorization capability, not audit data. The Decision API must project the server response into `AuditDecisionResult` and remove `confirmToken` before calling any audit sink, including console and file sinks.

## Tetris MVP audit example

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
    "inputs": [
      { "signal": { "key": "tetris.boardPressure" }, "value": 0.82 },
      { "signal": { "key": "tetris.currentLevel" }, "value": 3 },
      { "signal": { "key": "tetris.recentPlacementTimeMs" }, "value": 1420 },
      { "signal": { "key": "tetris.recoveryFailures" }, "value": 2 }
    ],
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
  "controlTarget": {
    "type": "cohort",
    "id": "new_players"
  },
  "stateSummary": {
    "decisionMode": "strategy",
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

## MVP non-goals

- Full audit query API.
- Long-term retention policy.
- LLM-generated explanations.
- Redaction pipeline beyond avoiding sensitive raw telemetry in runtime responses.
