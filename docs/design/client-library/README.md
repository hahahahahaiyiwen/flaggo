# Client Library Design

## Purpose

The client library is the developer-facing integration point for flaggo. It lets application code declare decision keys and definitions, emit decision evidence, pass runtime context, request `RuntimeDecisionResult` values, and safely apply returned values or fallbacks.

For the hero scenario, the first client library target is TypeScript for the Tetris frontend.

## Design goals

- Make AI-native runtime decisioning feel like a normal application primitive.
- Keep the first API small and explicit.
- Hide telemetry plumbing without hiding telemetry semantics.
- Make decision keys and definitions discoverable in code.
- Make fallback behavior part of the decision declaration.
- Integrate with OpenTelemetry where configured.
- Avoid forcing developers to build metrics aggregation, policy checks, or audit correlation manually.
- Keep SDK interfaces stable while server-side strategies, evidence, and intelligence evolve.
- Keep definition synchronization language-neutral: SDK declarations can generate a definition bundle, but the control plane must also support manifest-first, registry-first, and direct REST-client workflows.

MVP implementation guidance: [MVP Implementation Guide](../../IMPLEMENTATION_GUIDE.md).
Shared contract reference: [Shared Contracts](../shared-contracts/README.md).

## Developer mental model

```text
development:
  declare adaptive value

build/release:
  produce or reference definition bundle
  validate/apply bundle
  receive registration receipt

deployment:
  attach expected contract/build identity to workload

runtime:
  get value with live context
  observe outcome
```

The application-facing loop should still feel like:

```text
declare -> decide by observing
```

## Initial responsibilities

The client library should support:

1. **Client configuration**
   - Flaggo runtime API base URL.
   - Application ID.
   - Environment.
   - API version.
   - Expected definition ID/digest/revision when available.
   - Telemetry mode.
   - OpenTelemetry export settings.

2. **Decision declaration**
   - Define decision key.
   - Define result type: boolean, number, or string.
   - Define default safe value.
   - Define range or allowed values.
   - Define optimization intent.
   - Define safety preset or advanced policy.

3. **Scoped decision request**
   - Pass runtime context.
   - Pass concrete target identifiers for the configured `inference.target` and signal target hierarchy.
   - Call the versioned runtime Decision API.
   - Receive the runtime decision value directly in the basic path.

4. **Decision-linked evidence emission**
   - Emit typed domain events and metrics through the SDK.
   - Automatically attach decision, target, value, audit, timestamp, and definition identity when available.
   - Allow advanced users to define typed events and evidence metrics explicitly.

5. **Fallback handling**
   - Use explicit fallback when service is unavailable.
   - Distinguish resolution fallback from decision fallback.
   - Use explicit fallback when response says decision fallback was required.
   - Preserve application behavior when decisioning fails closed.

6. **Definition bundle support**
   - Generate or reference a canonical `DecisionDefinitionBundle` in code-first workflows.
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
    expectedContractDigest: process.env.FLAGGO_CONTRACT_DIGEST,
    expectedRevision: process.env.FLAGGO_CONTRACT_REVISION,
    buildId: process.env.BUILD_ID,
    deploymentId: process.env.DEPLOYMENT_ID
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

The SDK should send compact expected definition identity on runtime decision calls when configured. It should not send the full definition bundle with normal runtime requests.

## API interaction model

The client library should interact with three categories of endpoints, but only the runtime decision endpoint is required for the first slice.

| Area | Client behavior |
|---|---|
| **Runtime Decision API** | Calls `/v1/decisions/{decisionKey}:decide` with runtime context and compact expected definition identity when application code requests a decision. |
| **Telemetry ingestion** | Emits telemetry through OpenTelemetry-compatible export when configured. The SDK should not invent a custom telemetry transport unless needed for direct/demo mode. |
| **Definition tooling / Management APIs** | Generates, validates, or applies canonical definition bundles outside normal runtime. This should be explicit, language-neutral, and not an accidental startup side effect. |

Design rule:

> Runtime decision calls are part of application execution; definition bundle validation and registration are part of build, release, deployment, GitOps, or operator workflow.

## Software lifecycle roles

The SDK has different responsibilities at different stages. It should not be required in every stage.

| Stage | What happens | SDK role | Non-SDK path |
| --- | --- | --- | --- |
| Development | Developer declares decision keys, events, metrics, and fallback. | Provide ergonomic TypeScript declarations, local fallback, and typed decision calls. | Author `flaggo.decision-definition-bundle.json` or configure decision key in registry. |
| Build | Definition artifact is produced or selected for that build. | Optional extractor emits canonical `DecisionDefinitionBundle`, `definitionDigest`, and build metadata. | Bundle is maintained as JSON/YAML or exported from registry/platform tooling. |
| CI/release | Bundle is validated/applied/promoted. | No runtime SDK required; generated bundle is just an input artifact. | `flaggo contracts validate/apply`, GitHub Action, GitOps reconciler, or platform pipeline. |
| Deployment | Accepted definition/build identity is attached to each workload version. | SDK can read env vars or generated constants. | Container labels, deployment annotations, injected env vars, or HTTP headers for REST clients. |
| Runtime | Application asks for decisions and emits telemetry. | Send runtime context, expected contract/build identity, and telemetry; apply fallback when needed. | Direct REST client sends the same compact identity and runtime context. |
| Observe/operate | Teams inspect contract drift, fallback, and strategy outcomes. | Expose response fields and emit diagnostics. | Operator console, audit API, logs, metrics, deployment checks. |

This separation lets TypeScript be the first ergonomic SDK while preserving polyglot and open-source-native portability.

### Basic decision and runtime value

```ts
export const boardPressureSignal = flaggo.metric.number({
  key: "tetris.boardPressure",
  range: [0, 1]
});

export const recentPlacementTimeMsSignal = flaggo.metric.number({
  key: "tetris.recentPlacementTimeMs",
  unit: "ms"
});

export const recoveryFailuresSignal = flaggo.metric.number({
  key: "tetris.recoveryFailures"
});

export const currentLevelSignal = flaggo.metric.number({
  key: "tetris.currentLevel"
});

export const piecePlacedEvent = flaggo.event({
  key: "tetris.piecePlaced",
  fields: {
    placementTimeMs: flaggo.number({ unit: "ms" }),
    hardDrop: flaggo.boolean()
  }
});

export const sessionEndedEvent = flaggo.event({
  key: "tetris.sessionEnded",
  fields: {
    endReason: flaggo.string(),
    durationSeconds: flaggo.number({ unit: "s" })
  }
});

export const earlyLossRateSignal = flaggo.metric.derived({
  key: "tetris.earlyLossRate",
  type: "number",
  from: sessionEndedEvent,
  aggregation: "rate(endReason == 'early_loss')",
  window: "24h"
});

export const hardDropRateSignal = flaggo.metric.derived({
  key: "tetris.hardDropRate",
  type: "number",
  from: piecePlacedEvent,
  aggregation: "rate(hardDrop == true)",
  window: "24h"
});

const dropIntervalDecision = await flaggo.tune.number("tetris.dropInterval", {
  targetHierarchy: ["session", "user", "cohort", "global"],
  signals: {
    evidence: [
      piecePlacedEvent,
      sessionEndedEvent,
      earlyLossRateSignal,
      hardDropRateSignal
    ]
  },
  intent: {
    type: "metric-objective",
    primary: { signal: earlyLossRateSignal, direction: "minimize" },
    secondary: [
      { signal: hardDropRateSignal, direction: "target", target: 0.45 },
      { signal: recentPlacementTimeMsSignal, direction: "minimize" }
    ],
    rationale: "Keep gameplay challenging but playable while reducing early frustration."
  },
  inference: {
    target: "session",
    inputs: [
      boardPressureSignal.input(boardPressure),
      recentPlacementTimeMsSignal.input(recentPlacementTimeMs),
      recoveryFailuresSignal.input(recoveryFailures),
      currentLevelSignal.input(game.level)
    ],
    fallbackOrder: ["cohort", "global"]
  },
  output: {
    default: 800,
    range: [200, 1500],
    step: 50
  },
  policy: {
    maxDelta: 50,
    cooldown: "20s",
    minSampleSize: 30,
    minEvidenceQuality: 0.7,
    maxModelUncertainty: 0.35
  },
  requestedApproval: "automatic",
  context: {
    session: flaggo.target.session(sessionId),
    user: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort),
    deviceType: device.type
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
await flaggo.exposures.confirm(dropIntervalDecision.decisionId);
```

The basic API keeps the `flaggo.tune.number(...)` SDK surface but returns a number decision object. The application applies the plain numeric value via `.value`, and the receipt carries `decisionId`/confirmation metadata for exposure attribution. Signal schemas are defined once near producers and reused through typed handles. Bound inference inputs combine a signal declaration reference with its current value, while typed target wrappers combine target schema with the current ID. Tooling partitions this object into an immutable extracted definition and a compact runtime request; runtime values never enter the definition digest. Emitting a signal does not associate it with every decision in the program.

### Basic evidence emission

```ts
boardPressureSignal.emit(boardPressure);
recentPlacementTimeMsSignal.emit(recentPlacementTimeMs);
recoveryFailuresSignal.emit(recoveryFailures);

piecePlacedEvent.emit({
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});

sessionEndedEvent.emit({
  endReason,
  durationSeconds
});
```

The SDK and telemetry pipeline should attach decision context automatically when possible:

- decision key,
- decision definition revision,
- returned value,
- runtime target,
- resolved control target when known,
- decision/audit correlation ID,
- timestamp,
- definition revision or digest,
- application/build identity.

An explicit decision-scoped observation helper can exist as shorthand for advanced users, but it should not be required in the hero path.

### Advanced evidence and governance

```ts
const dropIntervalDecision = await flaggo.tune.number("tetris.dropInterval", {
  definition: {
    targetHierarchy: ["session", "user", "cohort", "global"],
    signals: {
      evidence: [
        piecePlacedEvent,
        sessionEndedEvent,
        earlyLossRateSignal,
        hardDropRateSignal
      ]
    },
    intent: {
      type: "metric-objective",
      primary: { signal: earlyLossRateSignal, direction: "minimize" },
      secondary: [
        { signal: hardDropRateSignal, direction: "target", target: 0.45 },
        { signal: recentPlacementTimeMsSignal, direction: "minimize" }
      ],
      rationale: "Keep the game challenging while reducing early frustration."
    },
    inference: {
      target: "session",
      inputs: [
        boardPressureSignal,
        recentPlacementTimeMsSignal,
        recoveryFailuresSignal,
        currentLevelSignal
      ],
      fallbackOrder: ["cohort", "global"]
    },
    output: {
      default: 800,
      range: [200, 1500],
      step: 50
    },
    requestedApproval: "automatic",
    policy: {
      maxDelta: 50,
      cooldown: "20s",
      minSampleSize: 30,
      minEvidenceQuality: 0.7,
      maxModelUncertainty: 0.35
    },
    context: {
      sessionId: { type: "string", target: "session" },
      userId: { type: "string", target: "user" },
      cohort: { type: "string", target: "cohort" },
      deviceType: "string"
    }
  },
  context: {
    sessionId,
    userId,
    cohort: playerCohort,
    deviceType: device.type
  },
  inputs: [
    boardPressureSignal.input(boardPressure),
    recentPlacementTimeMsSignal.input(recentPlacementTimeMs),
    recoveryFailuresSignal.input(recoveryFailures),
    currentLevelSignal.input(game.level)
  ]
});
```

This is the explicit advanced form. It preserves separate reusable definition, context, and input values when build tooling cannot infer them from one code-first expression or when the definition is managed independently.

Typed signal handles remain available wherever values are produced:

```ts
piecePlacedEvent.emit({
  placementTimeMs,
  hardDrop: placementMethod === "hard_drop"
});

boardPressureSignal.emit(boardPressure);

// Derived signals such as hardDropRateSignal are declared once
// and can be referenced by any decision allowed to use them.
```

The library should make fallback semantics easy to inspect:

```ts
if (decision.fallback.decisionFallbackUsed) {
  console.warn("Using safe fallback", decision.fallback.reason);
}

if (decision.fallback.resolutionFallbackUsed) {
  console.info(
    `Decision resolved using ${decision.controlTarget?.type}:${decision.controlTarget?.id}`
  );
}
```

## Expected decision response shape

The library should expose the shared runtime response shape with a developer-friendly alias:

```ts
type DecisionResult<T> = DecideResponse<T>;
```

`confidence` is `null` when Flaggo returns a static decision fallback because no evidence-backed decision was approved. If only target/evidence resolution fallback happened, confidence should still be present and should refer to the returned decision's evidence views and control target.

`decisionMode` tells the application how the value was produced without exposing internal implementation details. For the Tetris adaptive MVP, the expected mode is usually `strategy`: the server executed an approved strategy against live runtime context and returned an immediate numeric value.

`definition.integrity` should tell the application whether the runtime response was produced under a verified or known definition identity, or whether the response fell back because the definition was unknown, retired, or conflicting.

The basic `tune.number(...)` helper returns `DecisionReceipt<number>` for attribution while preserving the original SDK method name. An advanced detailed helper should expose the full `DecisionResult<T>`.

## MVP SDK interfaces

The first TypeScript SDK should keep a small interface:

```ts
interface IFlaggoClient {
  tune: ITuneBuilder;
  events: IEventBuilder;
  metrics: IMetricBuilder;
  exposures: IExposureBuilder;
  definitions: IDefinitionBundleProvider;
}

interface ITuneBuilder {
  number(name: string, request: NumberTuneRequest): Promise<DecisionReceipt<number>>;

  numberDetailed(name: string, request: NumberTuneRequest): Promise<DecisionResult<number>>;
}

interface IExposureBuilder {
  confirm(decisionId: string): Promise<{ exposureId: string }>;
}

type DecisionReceipt<T extends DecisionValue = DecisionValue> = {
  value: T;
  decisionId: string;
  confirmToken?: string;
};

type NumberTuneRequest =
  | CodeFirstNumberTuneRequest
  | ExplicitNumberTuneRequest;

type CodeFirstNumberTuneRequest = {
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: BoundInferenceDeclaration;
  intent: DecisionIntent;
  output: NumberOutputContract;
  policy: InlinePolicy;
  requestedApproval?: RequestedApprovalMode;
  context: BoundRuntimeContext;
};

type ExplicitNumberTuneRequest = {
  definition: AdvancedNumberTuneDefinition;
  context: RuntimeContext;
  inputs?: SignalInput[];
};

type AdvancedNumberTuneDefinition = {
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: InferenceDeclaration;
  intent: DecisionIntent;
  output: NumberOutputContract;
  policy?: InlinePolicy;
  requestedApproval?: RequestedApprovalMode;
  context: RuntimeContextSchema;
};

type RequestedApprovalMode = "automatic" | "human" | "policy-default";

type NumberOutputContract = {
  default: number;
  range: [number, number];
  step?: number;
};

type RuntimeContextSchema = Record<
  string,
  | "string"
  | "number"
  | "boolean"
  | { type: "string" | "number" | "boolean"; target?: string }
>;

type RuntimeContextValue = string | number | boolean | null;

type SignalInput = {
  signal: InferenceSignalHandle;
  value: RuntimeContextValue;
};
type RuntimeContext = Record<string, RuntimeContextValue>;
type BoundRuntimeContext = Record<
  string,
  RuntimeContextValue | TargetBinding
>;

type TargetBinding = {
  target: string;
  value: string;
};

type DecisionSignalReferences = {
  evidence?: SignalHandle[];
  guardrails?: SignalHandle[];
};

type SignalHandle = {
  key: string;
  schemaDigest?: string;
  emit?: (input: unknown) => void;
};

type InferenceSignalHandle = SignalHandle & {
  input: (value: unknown) => SignalInput;
};

type InferenceDeclaration = {
  target: "global" | "segment" | "cohort" | "user" | "session" | "level" | string;
  inputs?: InferenceSignalHandle[];
  fallbackOrder?: string[];
};

type BoundInferenceDeclaration = {
  target: "global" | "segment" | "cohort" | "user" | "session" | "level" | string;
  inputs?: SignalInput[];
  fallbackOrder?: string[];
};

type DecisionIntent =
  | NaturalLanguageIntent
  | MetricObjectiveIntent;

type NaturalLanguageIntent = {
  type: "natural-language";
  text: string;
};

type MetricObjectiveIntent = {
  type: "metric-objective";
  primary: MetricObjective;
  secondary?: MetricObjective[];
  rationale?: string;
};

type MetricObjective = {
  signal: SignalHandle;
  direction: "minimize" | "maximize" | "target";
  target?: number;
};

interface IDefinitionBundleProvider {
  exportBundle(): DecisionDefinitionBundle;
  getExpectedIdentity(): ContractIdentity | undefined;
}
```

The SDK should not implement policy, strategy selection, async intelligence, or server state. Its responsibilities are definition extraction from static request fields, telemetry, optional definition bundle export, runtime request, compact definition identity propagation, typed response, and local fallback when the service is unavailable.

In the basic path, `default` is the singular safe fallback value. During bundle generation, the SDK can compile it into the lower-level action-space default and fallback definition required by the registry/runtime model.

## Contract authoring, bundle, and registration

The client library may support code-first decision declarations, but Flaggo should not require production applications to mutate management state during normal runtime startup.

Decision declarations should be extractable into a canonical `DecisionDefinitionBundle`. Build, release, deployment, GitOps, or operator tooling can validate and apply that bundle to Flaggo management APIs.

Recommended code-first lifecycle:

```text
write declaration in code
  -> extract flaggo.contract-bundle.json
  -> validate/apply bundle with contract tooling
  -> receive registration receipt
  -> deploy app with expected contract/build identity
  -> runtime only calls decide and emits telemetry
```

Supported authoring modes:

| Mode | Behavior |
| --- | --- |
| `code-first` | SDK declarations generate or contribute to a canonical `DecisionDefinitionBundle`. |
| `bundle-first` | Application or platform uses hand-authored JSON/YAML bundle; SDK may only reference identity. |
| `registry-first` | Decision key is managed in Flaggo registry or operator tooling; SDK references pre-registered decision definition and expected identity. |
| `local-only` | Use declarations and fallback locally without remote validation. Useful for demos and early development. |

The first slice should support TypeScript `code-first` generation for the Tetris demo and direct `bundle-first` REST compatibility at the definition/API layer.

Resource lifecycle is owned by the Contract Registry. Client tooling should create or validate resources through bundle sync, but should not hard-delete missing resources. Missing declarations should become deprecation candidates, not deletes.

### Versioning UX

Developers should not need to manually version every decision definition during normal development. The SDK and registry can hide most version management behind validation/apply:

```text
developer names the decision: tetris.dropInterval
  -> tooling extracts canonical semantics
  -> registry compares with existing definition
  -> unchanged or metadata-only: keep same definition identity
  -> semantic conflict: reject overwrite or mint approved new semantic identity
```

The developer-facing name can stay stable for the common case. When semantics change in a way that could make old decision state unsafe, tooling should make the change explicit in PR/CI output:

```text
tetris.dropInterval
semantic change detected: added required signal recoveryFailures
state reuse: no
telemetry reuse: boardPressure and placementTimeMs evidence can be reused
new evidence warmup: recoveryFailures
suggested definition: tetris.dropInterval@2
```

This keeps the normal UX simple while still preventing accidental sharing of active strategies across incompatible contracts.

## Runtime definition identity

The runtime SDK should send compact expected definition identity when available:

```ts
const decision = await dropInterval.decide({
  expectedDefinitionId: flaggo.definitions.getExpectedIdentity()?.definitionId,
  expectedDefinitionDigest: flaggo.definitions.getExpectedIdentity()?.definitionDigest,
  expectedDefinitionRevision: flaggo.definitions.getExpectedIdentity()?.revision,
  buildId: flaggo.definitions.getExpectedIdentity()?.buildId,
  deploymentId: flaggo.definitions.getExpectedIdentity()?.deploymentId,
  runtimeContext: {
    userId,
    sessionId,
    boardPressure,
    recentPlacementTimeMs
  }
});
```

The full `DecisionDefinitionBundle` should not be sent with each runtime request. Runtime identity can come from:

- SDK build metadata,
- generated constants,
- environment variables,
- deployment annotations or injected config,
- direct REST headers/body fields.

Different builds of the same service can be deployed at the same time. The SDK should treat expected definition identity as build/deployment metadata attached to each workload.

If the Decision API reports an unknown or conflicting definition, the SDK should expose the fallback response clearly. It may log or emit diagnostics, but it should not hide the fallback behind a success-shaped local value.

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

- What exact `DecisionDefinitionBundle` JSON schema should the TypeScript extractor emit?
- Should browser SDKs send definition identity on every decision request or use a cached handshake?
- How much local evidence should the browser compute before sending telemetry?
- Should fallback be applied automatically by the library or explicitly by app code?
- How should TypeScript types be generated from server-side contracts?
- Should direct telemetry mode exist only in local/demo environments?

## First slice

For the Tetris hero scenario, the first client library design should support:

- TypeScript only.
- Number decision definitions.
- Basic `tune.number(...)` call returning a number decision receipt with a plain numeric `.value`.
- Safety preset support, starting with `gradual`.
- Signal declarations plus normal domain event/OpenTelemetry emission.
- Advanced domain event and metric declarations as optional evidence mode.
- Runtime context.
- Concrete target identifiers in the tune request.
- Session/user/cohort/global target hierarchy.
- Singular default/fallback value.
- DecisionDefinitionBundle generation or reference.
- Expected definition digest/revision propagation.
- Decision API call with a typed response.
- Runtime API path versioning through `/v1`.
- OpenTelemetry telemetry mode.
- No production runtime registration side effects.
