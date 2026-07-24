# Client Library Design

## Purpose

The client library is the developer-facing integration point for flaggo. It lets application code declare decision surfaces, emit telemetry evidence, pass runtime context, ask for governed decisions, and safely apply returned values or fallbacks.

For the hero scenario, the first client library target is TypeScript for the Tetris frontend.

## Design goals

- Make AI-native runtime decisioning feel like a normal application primitive.
- Keep the first API small and explicit.
- Hide telemetry plumbing without hiding telemetry semantics.
- Make decision surfaces discoverable in code.
- Make fallback behavior part of the decision declaration.
- Integrate with OpenTelemetry where configured.
- Avoid forcing developers to build metrics aggregation, policy checks, or audit correlation manually.

## Developer mental model

```text
create client
  -> define domain events
  -> define evidence metrics
  -> declare decision surface
  -> emit telemetry
  -> ask for decision with runtime context
  -> apply returned value or fallback
```

## Initial responsibilities

The client library should support:

1. **Client configuration**
   - Flaggo runtime API base URL.
   - Application ID.
   - Environment.
   - API version.
   - Telemetry mode.
   - OpenTelemetry export settings.

2. **Domain event definition**
   - Define event names.
   - Define expected properties.
   - Emit typed event payloads.

3. **Evidence metric definition**
   - Define metric meaning from domain events.
   - Define aggregation window.
   - Define desired direction or target.
   - Define evidence scope hints.

4. **Decision surface declaration**
   - Define surface name.
   - Define result type: boolean, number, or string.
   - Define action space.
   - Define goals.
   - Define policy hints or referenced policy.
   - Define fallback contract.
   - Define scope hierarchy or referenced scope profile.

5. **Decision request**
   - Pass runtime context.
   - Optionally pass requested scope.
   - Call the versioned runtime Decision API.
   - Receive governed decision response.

6. **Fallback handling**
   - Use explicit fallback when service is unavailable.
   - Distinguish resolution fallback from decision fallback.
   - Use explicit fallback when response says decision fallback was required.
   - Preserve application behavior when decisioning fails closed.

7. **Optional contract registration**
   - In development, the library may help register or validate decision surfaces against management APIs.
   - In production, contract registration may be a build/deploy-time concern instead of a runtime side effect.

## Example shape

This is intent-level pseudo-code, not final API.

```ts
const flaggo = createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  apiVersion: "v1",
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000
  },
  registration: {
    mode: "validate-only"
  }
});
```

## API interaction model

The client library should interact with three categories of endpoints, but only the runtime decision endpoint is required for the first slice.

| Area | Client behavior |
|---|---|
| **Runtime Decision API** | Calls `/v1/decisions/{surface}:decide` when application code requests a decision. |
| **Telemetry ingestion** | Emits telemetry through OpenTelemetry-compatible export when configured. The SDK should not invent a custom telemetry transport unless needed for direct/demo mode. |
| **Management APIs** | Optionally registers or validates decision surfaces, contracts, policies, and scope hierarchy. This should be explicit, not an accidental runtime side effect. |

Design rule:

> Runtime decision calls are part of application execution; management registration is part of configuration/deployment workflow.

### Domain events

```ts
const hardDropPressed = flaggo.events.define("hard_drop_pressed", {
  properties: {
    userId: "string",
    sessionId: "string",
    pieceType: "string",
    dropInterval: "number",
    level: "number"
  }
});

const piecePlaced = flaggo.events.define("piece_placed", {
  properties: {
    userId: "string",
    sessionId: "string",
    placementTimeMs: "number",
    placementMethod: "string",
    dropInterval: "number"
  }
});
```

```ts
function onHardDrop(piece: Tetromino) {
  gameEngine.hardDrop();

  hardDropPressed.emit({
    userId,
    sessionId,
    pieceType: piece.type,
    dropInterval: gameEngine.config.dropInterval,
    level: gameEngine.level
  });
}
```

### Evidence metrics

```ts
const gameEvidence = flaggo.metrics.define({
  hardDropRate: {
    numerator: hardDropPressed.count(),
    denominator: piecePlaced.count(),
    scope: "session",
    window: "2m",
    direction: "target",
    target: 0.45
  }
});
```

### Decision surface

```ts
const dropInterval = flaggo.decision.number("tetris.dropInterval", {
  scopeHierarchy: ["session", "user", "segment", "global"],
  actionSpace: {
    min: 200,
    max: 1500,
    step: 50,
    default: 800
  },
  goals: [
    gameEvidence.hardDropRate.near(0.45)
  ],
  policy: {
    maxDelta: 50,
    cooldown: "5m",
    minSampleSize: 30,
    minConfidence: 0.7
  },
  fallback: {
    value: 800,
    strategy: "use-default"
  }
});
```

### Decision call

```ts
const decision = await dropInterval.decide({
  requestedScope: {
    type: "session",
    id: sessionId
  },
  runtimeContext: {
    userId,
    sessionId,
    currentLevel,
    deviceType
  }
});

gameEngine.updateConfig({ dropInterval: decision.value });
```

The library should make fallback semantics easy to inspect:

```ts
if (decision.fallback.decisionFallbackUsed) {
  console.warn("Using safe fallback", decision.fallback.reason);
}

if (decision.fallback.resolutionFallbackUsed) {
  console.info(
    `Decision resolved from ${decision.evidenceScope?.type}:${decision.evidenceScope?.id}`
  );
}
```

## Expected decision response shape

The library should expose a simple typed response:

```ts
type DecisionResult<T> = {
  value: T;
  valueType: "boolean" | "number" | "string";
  confidence: number | null;
  reason?: string;
  auditId: string;
  requestedScope?: {
    type: string;
    id: string;
  };
  resolvedScope: {
    type: string;
    id: string;
  };
  evidenceScope?: {
    type: string;
    id: string;
  };
  fallback: {
    resolutionFallbackUsed: boolean;
    decisionFallbackUsed: boolean;
    reason: string | null;
  };
  policy: {
    result: "approved" | "blocked" | "fallback";
    reasons: string[];
  };
};
```

`confidence` is `null` when Flaggo returns a static decision fallback because no evidence-backed decision was approved. If only resolution fallback happened, confidence should still be present and should refer to the returned decision at `evidenceScope`.

## Contract declaration and registration

The client library may support code-first decision declarations, but Flaggo should not require production applications to mutate management state during normal runtime startup.

Decision declarations should be extractable into a resource ownership manifest. Build or deployment tooling can sync that manifest to Flaggo management APIs.

Recommended lifecycle:

```text
write declaration in code
  -> extract ownership manifest
  -> sync/validate with Contract Registry
  -> deploy app
  -> runtime only calls decide and emits telemetry
```

Recommended modes:

| Mode | Behavior |
|---|---|
| `local-only` | Use declarations only for local typing/fallback behavior; no server registration. |
| `validate-only` | Compare local declarations with server contracts and warn/fail on mismatch. |
| `register-dev` | Register or update surfaces/contracts automatically for development environments. |
| `disabled` | Application only calls pre-registered surfaces. |

The first slice can support `validate-only` as the target behavior and leave automatic registration for later.

Resource lifecycle is owned by the Contract Registry. Client tooling should create or validate resources through manifest sync, but should not hard-delete missing resources. Missing declarations should become deprecation candidates, not deletes.

## Telemetry behavior

The client library should expose a domain-event API, but the transport should prefer OpenTelemetry.

Initial telemetry modes:

| Mode | Behavior |
|---|---|
| `opentelemetry` | Emit via OTel-compatible exporter/collector. |
| `direct` | Send to a Flaggo development/demo ingestion endpoint. |
| `disabled` | Do not emit telemetry; decisions rely on existing server evidence or fallback. |

Domain events should be developer-friendly. The SDK may map them to structured logs, span events, or metric measurements under the hood.

## Open design questions

- Should decision declarations be code-only, server-registered, or both?
- Should the client library register surfaces automatically at startup?
- How much local evidence should the browser compute before sending telemetry?
- Should fallback be applied automatically by the library or explicitly by app code?
- How should TypeScript types be generated from server-side contracts?
- Should production clients be allowed to use `register-dev`, or should management writes require separate tooling?
- Should direct telemetry mode exist only in local/demo environments?

## First slice

For the Tetris hero scenario, the first client library design should support:

- TypeScript only.
- Number decision surfaces.
- Domain event emission.
- Simple evidence metric declarations.
- Runtime context.
- Session/user/segment/global scope hierarchy.
- Explicit fallback value.
- Decision API call with a typed response.
- Runtime API path versioning through `/v1`.
- OpenTelemetry telemetry mode.
- Validate-only contract registration mode.
