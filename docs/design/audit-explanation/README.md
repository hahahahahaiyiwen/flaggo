# Audit and Explanation Design

## Purpose

Audit records make Flaggo decisions reconstructable. Explanation text makes them understandable to operators and developers.

For the MVP, audit should be local-first and simple: console, file, or in-memory records are enough if every decision response receives an `auditId`.

Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## MVP responsibility

Audit records should capture:

- request metadata,
- decision surface,
- requested/resolved/evidence scopes,
- contract version,
- runtime context summary,
- evidence snapshot summary,
- active value or active strategy,
- candidate value,
- policy result,
- returned value,
- fallback usage,
- reason text.

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
- evidence quality,
- scope resolution fallback reason.

## Runtime behavior

```text
Decision API
  -> builds audit record from request, state, evidence, candidate, policy, response
  -> writes through IAuditSink
  -> includes auditId in DecideResponse
```

If audit writing fails, the MVP should fail safe. For local development, surfacing the error is preferable to silently returning unaudited decisions.

## Tetris MVP audit example

```json
{
  "surface": "tetris.dropInterval",
  "contractVersion": "1",
  "resolvedScope": {
    "type": "segment",
    "id": "new_players"
  },
  "stateSummary": {
    "decisionMode": "strategy",
    "strategyId": "strategy-tetris-new-players-v1"
  },
  "policy": {
    "result": "approved",
    "reasons": [],
    "appliedConstraints": ["number-bounds", "max-delta", "cooldown"]
  },
  "reason": "Approved strategy slowed the drop interval because board pressure was high and recent placement time was slow."
}
```

## MVP non-goals

- Full audit query API.
- Long-term retention policy.
- LLM-generated explanations.
- Redaction pipeline beyond avoiding sensitive raw telemetry in runtime responses.
