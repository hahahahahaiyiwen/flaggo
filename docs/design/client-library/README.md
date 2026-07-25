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
- Keep SDK interfaces stable while server-side strategies, evidence, and intelligence evolve.
- Keep contract synchronization language-neutral: SDK declarations can generate a contract bundle, but the control plane must also support manifest-first, registry-first, and direct REST-client workflows.

MVP implementation guidance: [MVP Implementation Guide](../../IMPLEMENTATION_GUIDE.md).
Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## Developer mental model

```text
development:
  declare adaptive value

build/release:
  produce or reference contract bundle
  validate/apply bundle
  receive registration receipt

deployment:
  attach expected digest/revision to workload

runtime:
  get value with live context
  observe outcome
```

The application-facing loop should still feel like:

```text
declare -> decide -> observe
```

## Initial responsibilities

The client library should support:

1. **Client configuration**
   - Flaggo runtime API base URL.
   - Application ID.
   - Environment.
   - API version.
   - Expected contract digest/revision when available.
   - Telemetry mode.
   - OpenTelemetry export settings.

2. **Decision declaration**
   - Define surface name.
   - Define result type: boolean, number, or string.
   - Define default safe value.
   - Define range or allowed values.
   - Define optimization intent.
   - Define safety preset or advanced policy.

3. **Scoped decision request**
   - Pass runtime context.
   - Bind scope once through helpers such as `forSession(...)`.
   - Call the versioned runtime Decision API.
   - Receive governed decision value and metadata.

4. **Decision-scoped observation**
   - Record outcomes through `decision.observe(...)`.
   - Automatically attach decision, scope, value, audit, timestamp, and contract identity when available.
   - Allow advanced users to define typed events and evidence metrics explicitly.

5. **Fallback handling**
   - Use explicit fallback when service is unavailable.
   - Distinguish resolution fallback from decision fallback.
   - Use explicit fallback when response says decision fallback was required.
   - Preserve application behavior when decisioning fails closed.

6. **Contract bundle support**
   - Generate or reference a canonical `ContractBundle` in code-first workflows.
   - Expose bundle digest/revision metadata to runtime calls.
   - Avoid production management writes from normal application startup.

## Example shape

This is intent-level pseudo-code, not final API.

```ts
const flaggo = createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  apiVersion: "v1",
  contract: {
    expectedDigest: process.env.FLAGGO_CONTRACT_DIGEST,
    expectedRevision: process.env.FLAGGO_CONTRACT_REVISION
  },
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000
  },
  contractAuthoring: {
    mode: "code-first"
  }
});
```

The SDK should send compact expected contract identity on runtime decision calls when configured. It should not send the full contract bundle with normal runtime requests.

## API interaction model

The client library should interact with three categories of endpoints, but only the runtime decision endpoint is required for the first slice.

| Area | Client behavior |
|---|---|
| **Runtime Decision API** | Calls `/v1/decisions/{surface}:decide` with runtime context and compact expected contract identity when application code requests a decision. |
| **Telemetry ingestion** | Emits telemetry through OpenTelemetry-compatible export when configured. The SDK should not invent a custom telemetry transport unless needed for direct/demo mode. |
| **Contract tooling / Management APIs** | Generates, validates, or applies canonical contract bundles outside normal runtime. This should be explicit, language-neutral, and not an accidental startup side effect. |

Design rule:

> Runtime decision calls are part of application execution; contract bundle validation and registration are part of build, release, deployment, GitOps, or operator workflow.

## Software lifecycle roles

The SDK has different responsibilities at different stages. It should not be required in every stage.

| Stage | What happens | SDK role | Non-SDK path |
| --- | --- | --- | --- |
| Development | Developer declares surfaces, events, metrics, and fallback. | Provide ergonomic TypeScript declarations, local fallback, and typed decision calls. | Author `flaggo.contract-bundle.json` or configure surface in registry. |
| Build | Contract artifact is produced or selected. | Optional extractor emits canonical `ContractBundle` and digest. | Bundle is maintained as JSON/YAML or exported from registry/platform tooling. |
| CI/release | Bundle is validated/applied/promoted. | No runtime SDK required; generated bundle is just an input artifact. | `flaggo contracts validate/apply`, GitHub Action, GitOps reconciler, or platform pipeline. |
| Deployment | Accepted contract identity is attached to workload. | SDK can read env vars or generated constants. | Container labels, deployment annotations, injected env vars, or HTTP headers for REST clients. |
| Runtime | Application asks for decisions and emits telemetry. | Send runtime context, expected digest/revision, and telemetry; apply fallback when needed. | Direct REST client sends the same compact identity and runtime context. |
| Observe/operate | Teams inspect contract drift, fallback, and strategy outcomes. | Expose response fields and emit diagnostics. | Operator console, audit API, logs, metrics, deployment checks. |

This separation lets TypeScript be the first ergonomic SDK while preserving polyglot and open-source-native portability.

### Basic decision declaration

```ts
const dropInterval = flaggo.tune.number("tetris.dropInterval", {
  default: 800,
  range: [200, 1500],
  step: 50,
  optimize: "challenging-but-playable",
  safety: "gradual"
});
```

### Basic decision call

```ts
const sessionDropInterval = dropInterval.forSession(sessionId);

const interval = await sessionDropInterval.get({
  level: game.level,
  boardPressure,
  recentPlacementTimeMs,
  recoveryFailures
});

gameEngine.updateConfig({ dropInterval: interval });
```

The SDK should infer requested scope from `forSession(sessionId)` and attach expected contract identity when configured. Direct REST clients can provide the same compact identity explicitly.

### Basic observation

```ts
sessionDropInterval.observe("piece_placed", {
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});

sessionDropInterval.observe("session_ended", {
  reason: endReason,
  durationSeconds
});
```

The SDK should attach decision context automatically when possible:

- surface,
- returned value,
- requested/resolved scope,
- decision/audit correlation ID,
- timestamp,
- contract revision or digest.

### Advanced evidence and governance

```ts
const dropInterval = flaggo.tune.number("tetris.dropInterval", {
  default: 800,
  range: [200, 1500],
  step: 50,
  optimize: {
    primary: signals.earlyLossRate.minimize(),
    secondary: [
      signals.hardDropRate.near(0.45),
      signals.placementTimeMs.minimize()
    ]
  },
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minConfidence: 0.7
  }
});
```

Named typed events and reusable metrics remain available as advanced evidence mode:

```ts
const piecePlaced = flaggo.events.define("piece_placed", {
  properties: {
    placementTimeMs: "number",
    placementMethod: "string"
  }
});

const signals = flaggo.metrics.define({
  placementTimeMs: {
    value: piecePlaced.property("placementTimeMs").average(),
    window: "5m",
    direction: "minimize"
  }
});
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

The library should expose the shared runtime response shape with a developer-friendly alias:

```ts
type DecisionResult<T> = DecideResponse<T>;
```

`confidence` is `null` when Flaggo returns a static decision fallback because no evidence-backed decision was approved. If only resolution fallback happened, confidence should still be present and should refer to the returned decision at `evidenceScope`.

`decisionMode` tells the application how the value was produced without exposing internal implementation details. For the Tetris adaptive MVP, the expected mode is usually `strategy`: the server executed an approved strategy against live runtime context and returned an immediate numeric value.

`contract.integrity` should tell the application whether the runtime response was produced under a verified or compatible contract identity, or whether the response fell back because of drift.

The basic `get(...)` helper may return `T` for ergonomics, while an advanced `decide(...)` or `getDetailed(...)` helper should expose the full `DecisionResult<T>`.

## MVP SDK interfaces

The first TypeScript SDK should keep a small interface surface:

```ts
interface IFlaggoClient {
  decision: IDecisionBuilder;
  events: IEventBuilder;
  metrics: IMetricBuilder;
  contracts: IContractBundleProvider;
}

interface IDecisionBuilder {
  number(name: string, declaration: NumberTuneDeclaration): INumberDecision;
}

type NumberTuneDeclaration =
  | BasicNumberTuneDeclaration
  | AdvancedNumberTuneDeclaration;

type BasicNumberTuneDeclaration = {
  default: number;
  range: [number, number];
  step?: number;
  optimize: string;
  safety: "gradual" | "conservative" | "manual";
};

type AdvancedNumberTuneDeclaration = {
  default: number;
  range: [number, number];
  step?: number;
  optimize: string | AdvancedOptimizationGoal;
  safety?: "gradual" | "conservative" | "manual";
  policy?: InlinePolicy;
};

type AdvancedOptimizationGoal = {
  primary: EvidenceGoal;
  secondary?: EvidenceGoal[];
};

type EvidenceGoal = unknown;

interface INumberDecision {
  forSession(sessionId: string): IScopedNumberDecision;

  get(context: RuntimeContext): Promise<number>;

  getDetailed(context: RuntimeContext): Promise<DecisionResult<number>>;

  observe(eventName: string, payload: RuntimeContext): void;

  fallbackValue(): number;
}

interface IScopedNumberDecision {
  get(context: RuntimeContext): Promise<number>;

  getDetailed(context: RuntimeContext): Promise<DecisionResult<number>>;

  observe(eventName: string, payload: RuntimeContext): void;

  decide(request: {
    requestedScope?: ScopeRef;
    runtimeContext: RuntimeContext;
    correlationId?: string;
    expectedContractDigest?: string;
    expectedContractRevision?: string;
  }): Promise<DecisionResult<number>>;

  fallbackValue(): number;
}

interface IContractBundleProvider {
  exportBundle(): ContractBundle;
  getExpectedIdentity(): ContractIdentity | undefined;
}
```

The SDK should not implement policy, strategy selection, async intelligence, or server state. Its responsibilities are declaration, telemetry, optional contract bundle export, runtime request, compact contract identity propagation, typed response, and local fallback when the service is unavailable.

In the basic path, `default` is the singular safe fallback value. During bundle generation, the SDK can compile it into the lower-level action-space default and fallback contract required by the registry/runtime model.

## Contract authoring, bundle, and registration

The client library may support code-first decision declarations, but Flaggo should not require production applications to mutate management state during normal runtime startup.

Decision declarations should be extractable into a canonical `ContractBundle`. Build, release, deployment, GitOps, or operator tooling can validate and apply that bundle to Flaggo management APIs.

Recommended code-first lifecycle:

```text
write declaration in code
  -> extract flaggo.contract-bundle.json
  -> validate/apply bundle with contract tooling
  -> receive registration receipt
  -> deploy app with expected digest/revision
  -> runtime only calls decide and emits telemetry
```

Supported authoring modes:

| Mode | Behavior |
| --- | --- |
| `code-first` | SDK declarations generate or contribute to a canonical `ContractBundle`. |
| `bundle-first` | Application or platform uses hand-authored JSON/YAML bundle; SDK may only reference identity. |
| `registry-first` | Surface is managed in Flaggo registry or operator tooling; SDK references pre-registered surface and expected identity. |
| `local-only` | Use declarations and fallback locally without remote validation. Useful for demos and early development. |

The first slice should support TypeScript `code-first` generation for the Tetris demo and direct `bundle-first` REST compatibility at the contract/API layer.

Resource lifecycle is owned by the Contract Registry. Client tooling should create or validate resources through bundle sync, but should not hard-delete missing resources. Missing declarations should become deprecation candidates, not deletes.

## Runtime contract identity

The runtime SDK should send compact expected contract identity when available:

```ts
const decision = await dropInterval.decide({
  expectedContractDigest: flaggo.contracts.getExpectedIdentity()?.digest,
  expectedContractRevision: flaggo.contracts.getExpectedIdentity()?.revision,
  runtimeContext: {
    userId,
    sessionId,
    boardPressure,
    recentPlacementTimeMs
  }
});
```

The full `ContractBundle` should not be sent with each runtime request. Runtime identity can come from:

- SDK build metadata,
- generated constants,
- environment variables,
- deployment annotations or injected config,
- direct REST headers/body fields.

If the Decision API reports incompatible drift, the SDK should expose the fallback response clearly. It may log or emit diagnostics, but it should not hide the fallback behind a success-shaped local value.

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

- What exact `ContractBundle` JSON schema should the TypeScript extractor emit?
- Should browser SDKs send contract identity on every decision request or use a cached handshake?
- How much local evidence should the browser compute before sending telemetry?
- Should fallback be applied automatically by the library or explicitly by app code?
- How should TypeScript types be generated from server-side contracts?
- Should direct telemetry mode exist only in local/demo environments?

## First slice

For the Tetris hero scenario, the first client library design should support:

- TypeScript only.
- Number decision surfaces.
- Basic `tune.number(...)` declaration.
- Safety preset support, starting with `gradual`.
- Decision-scoped `observe(...)`.
- Advanced domain event and metric declarations as optional evidence mode.
- Runtime context.
- Scope helper such as `forSession(sessionId)`.
- Session/user/segment/global scope hierarchy.
- Singular default/fallback value.
- ContractBundle generation or reference.
- Expected contract digest/revision propagation.
- Decision API call with a typed response.
- Runtime API path versioning through `/v1`.
- OpenTelemetry telemetry mode.
- No production runtime registration side effects.
