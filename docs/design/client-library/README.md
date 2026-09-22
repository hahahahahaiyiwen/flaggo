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
- Keep definition synchronization language-neutral: SDK declarations can
  generate a definition bundle, but the control plane must also support
  bundle-first, registry-first, and direct REST-client workflows.

MVP implementation guidance: [MVP Implementation Guide](../../IMPLEMENTATION_GUIDE.md).
Shared contract reference: [Shared Contracts](../shared-contracts/README.md).
Phase 1 wire-contract proposal: [API Contract Proposal](../API_CONTRACT_PROPOSAL.md).
Control-plane/data-plane developer experience: [Control Plane and Data Plane UX](CONTROL_DATA_PLANE_UX.md).

## Developer mental model

```text
development:
  declare adaptive value

build/release:
  optionally produce or reference definition bundle

application/bootstrap startup (MVP):
  validate/apply bundle through the control-plane API
  obtain authenticated approval for declared initial authority
  wait for activation and receive a ready registration receipt
  initialize runtime binding

deployment:
  deploy application independently

runtime:
  require exact registered identity for remote decisions
  observe outcome
```

The application-facing loop should still feel like:

```text
declare -> approve -> decide -> observe
```

## Initial responsibilities

The client library should support:

1. **Client configuration**
   - Flaggo runtime API base URL.
   - Application ID.
   - Environment.
   - API version.
   - OAuth 2.0/OIDC credential provider and required data-plane/control-plane scopes.
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
   - Optionally attach an idempotency key for safe decide retries.
   - Treat cohort/segment identifiers as claims that the server may verify or replace.
   - Receive the runtime decision value directly in the basic path.

4. **Decision-linked evidence emission**
   - Emit typed domain events and metrics through the SDK.
   - Automatically attach decision, target, value, audit, timestamp, and definition identity when available.
   - Allow advanced users to define typed events and evidence metrics explicitly.

5. **Fallback handling**
   - Optionally use the code-declared fallback when the data plane is unavailable.
   - Distinguish resolution fallback from decision fallback.
   - Preserve audited governed fallback returned by the server.
   - Surface contract/configuration errors without local fallback.
   - Preserve application behavior when decisioning fails closed.

6. **Definition bundle support**
   - Generate or reference a canonical `DecisionDefinitionBundle` in code-first workflows.
   - Expose bundle digest/revision metadata to runtime calls.
   - For MVP, explicitly validate/apply the extracted bundle, authorize its
     initial authority, and wait for activation readiness during trusted
     application/bootstrap startup before enabling data-plane calls.
   - Keep management calls separate from decide and exposure operations.

## Example shape

This is intent-level pseudo-code, not final API.

```ts
const flaggo = await createFlaggoClient({
  serviceUrl: "https://flaggo.example.com",
  appId: "tetris-demo",
  environment: "dev",
  controlPlane: {
    mode: "startup-register",
    bundle: generatedDecisionBundle,
    credential: localBootstrapCredential
  },
  telemetry: {
    exporter: "opentelemetry",
    otlpEndpoint: "https://otel-collector.example.com",
    sampleRate: 1.0,
    flushIntervalMs: 5000
  },
  contractAuthoring: {
    mode: "code-first"
  },
  availabilityFallback: {
    mode: "local-default"
  }
});
```

Startup registration sends the extracted bundle once to the management API and
initializes compact expected identity from a ready receipt. A ready receipt for
a bundle-approved definition also identifies the derived proposal, activation,
state, generation, target, and strategy kind. Production decision calls send
only the definition identity. Availability fallback is disabled unless
explicitly configured; `local-default` uses the decision's code-declared
default only for recognized data-plane availability failures.

If apply returns `requires-approval`, startup raises a typed error containing
the `approvalRequestId` and does not initialize the data-plane client. After an
authorized reviewer approves the exact bundle snapshot, the service derives
the activation request and publishes state through expected-baseline
compare-and-swap. Retrying startup with the same bundle receives the original
ready receipt once activation succeeds.

If approval expires, retrying the same startup apply and deterministic key causes the server to revalidate and create one fresh linked approval request. Concurrent replicas receive that replacement request rather than minting independent approvals.

The credential shown is valid only for a trusted bootstrap environment. Production browser bundles must use another control-plane client experience because they cannot safely hold management credentials.

## API interaction model

The client library interacts with three categories of endpoints. Runtime decide and exposure confirmation form the data plane. Bundle management forms the control plane; for MVP, a trusted application/bootstrap startup client calls it before enabling runtime decisions.

| Area | Client behavior |
|---|---|
| **Runtime Decision API** | Calls `/v1/decisions/{decisionKey}:decide`, applies the returned value, and calls `/v1/exposures/{decisionId}:confirm` only when that value was applied or rendered. |
| **Telemetry ingestion** | Emits telemetry through OpenTelemetry-compatible export when configured. The SDK should not invent a custom telemetry transport unless needed for direct/demo mode. |
| **Definition tooling / Management APIs** | Generates, validates, or applies canonical definition bundles. MVP uses explicit startup registration; future clients may use CLI, CI/CD, GitOps, deployment hooks, or registry-first workflows. |

Design rule:

> Runtime decision calls are data-plane operations. Definition registration is a separate control-plane operation even when the MVP SDK coordinates it during application/bootstrap startup.

Application deployment itself remains developer-owned and can proceed without Polari tooling. In that case, data-plane calls fail until the exact expected definition is registered. See [Control Plane and Data Plane UX](CONTROL_DATA_PLANE_UX.md).

## Software lifecycle roles

The SDK has different responsibilities at different stages. It should not be required in every stage.

| Stage | What happens | SDK role | Non-SDK path |
| --- | --- | --- | --- |
| Development | Developer declares decision keys, events, metrics, and fallback. | Provide ergonomic TypeScript declarations, availability-fallback typing, and typed decision calls. | Author `flaggo.decision-definition-bundle.json` or configure decision key in registry. |
| Build | Definition artifact is produced or selected for that build. | Optional extractor emits the canonical `DecisionDefinitionBundle`, per-definition `contractDigest` values, and build metadata. | Bundle is maintained as JSON/YAML or exported from registry/platform tooling. |
| Application deployment | Code is deployed independently. | No Polari management action is implied by deployment itself. | Existing deployment mechanism remains unchanged. |
| Application/bootstrap startup | MVP atomically validates/applies the extracted bundle and receives the runtime binding. | Trusted SDK bootstrap acts as a control-plane client, then initializes the data-plane client. | Future alternatives include CLI, CI/CD, GitOps, init/deployment hooks, or registry-first tooling. |
| Runtime | Application asks for decisions and emits telemetry. | Send exact expected definition identity; surface contract errors; optionally apply local fallback only for configured availability failures. | Direct REST client sends the same compact identity and handles Problem Details. |
| Observe/operate | Teams inspect contract drift, fallback, and strategy outcomes. | Expose response fields and emit diagnostics. | Operator console, audit API, logs, metrics, deployment checks. |

This separation lets TypeScript be the first ergonomic SDK while preserving polyglot and open-source-native portability. Polari tooling can optimize control-plane publication without claiming ownership of application deployment.

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
  key: "tetris.earlyLossRate24h",
  type: "number",
  from: sessionEndedEvent,
  aggregation: "rate(endReason == 'early_loss')",
  window: "24h"
});

export const hardDropRateSignal = flaggo.metric.derived({
  key: "tetris.hardDropRate24h",
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
  lifecycle: {
    authorityMode: "bundle-approved",
    initialAuthority: {
      controlTarget: flaggo.target.cohort("new_players"),
      kind: "numeric-rule",
      rule: {
        threshold: 0.55,
        valueAtOrAbove: 850,
        valueBelow: 750,
        weightedInputs: [
          {
            signal: boardPressureSignal,
            minimum: 0,
            maximum: 1,
            weight: 0.45
          },
          {
            signal: recentPlacementTimeMsSignal,
            minimum: 0,
            maximum: 2000,
            weight: 0.25
          },
          {
            signal: recoveryFailuresSignal,
            minimum: 0,
            maximum: 5,
            weight: 0.20
          },
          {
            signal: currentLevelSignal,
            minimum: 0,
            maximum: 20,
            weight: 0.10
          }
        ]
      },
      rationale: "Initial deterministic Tetris behavior."
    }
  },
  policy: {
    maxDelta: 50
  },
  context: {
    sessionId: flaggo.target.session(sessionId),
    userId: flaggo.target.user(userId),
    cohort: flaggo.target.cohort(playerCohort)
  }
});

gameEngine.updateConfig({ dropInterval: dropIntervalDecision.value });
if (
  dropIntervalDecision.source === "server" &&
  dropIntervalDecision.exposure.confirmationRequired
) {
  await flaggo.exposures.confirm(
    dropIntervalDecision.decisionId,
    dropIntervalDecision.exposure.confirmToken
  );
}
```

The basic API keeps the `flaggo.tune.number(...)` SDK surface but returns a number decision object. The application applies the plain numeric value via `.value`. A server receipt carries `decisionId` and confirmation metadata; an SDK-local fallback receipt deliberately does not. Signal schemas are defined once near producers and reused through typed handles. Bound inference inputs combine a signal declaration reference with its current value, while typed target wrappers combine target schema with the current ID. Tooling partitions this object into an immutable extracted definition and a compact runtime request; runtime values never enter the definition digest. Emitting a signal does not associate it with every decision in the program.

### Policy authoring normalization

The shorthand `policy` object is authoring syntax, not the canonical policy contract. Extraction normalizes it deterministically:

| `PolicyAuthoring` field | Canonical constraint |
| --- | --- |
| `maxDelta` | `{ kind: "max-delta", value }` |
| `cooldown: "20s"` | `{ kind: "cooldown", seconds: 20 }` |
| `minSampleSize` | `{ kind: "min-sample-size", value }` |
| `minEvidenceQuality` | `{ kind: "min-evidence-quality", value }` |
| `maxModelUncertainty` | `{ kind: "max-model-uncertainty", value }` |
| `minExpectedOutcome` | `{ kind: "min-expected-outcome", value }` |
| `paused` | `{ kind: "pause", paused }` |

The result is `InlinePolicy { kind: "inline", constraints }`; constraints are duplicate-free and canonically sorted by `kind`. The explicit advanced form accepts canonical `PolicyReference | InlinePolicy` directly.

`output.range` and `output.step` remain action-space semantics. Extraction does
not duplicate them into synthesized policy constraints, so the code-first
Tetris declaration and its canonical bundle form hash identically.

### Extractable code-first subset

MVP extraction must be deterministic and intentionally conservative. The extractor accepts:

- a literal decision key passed directly to `flaggo.tune.*(...)`,
- object literals for definition semantics,
- literal arrays of directly imported signal handles,
- `signal.input(runtimeExpression)` for bound inference values,
- `flaggo.target.<kind>(runtimeExpression)` for bound target IDs,
- plain context properties only when the TypeScript checker resolves an exact supported primitive type,
- arbitrary runtime expressions inside the value position of signal/target bindings.

The extractor rejects static semantics built with:

- object or array spreads,
- computed property names,
- conditional definition fields or conditional signal inputs,
- loops or dynamically constructed signal arrays,
- helper-returned definition fragments,
- mutation of a declaration after construction,
- a non-literal or dynamically computed decision key.

If a plain context expression's type cannot be resolved unambiguously, authors must use an explicit typed context binding supplied by the SDK. Unsupported syntax is a build/tooling error, not a best-effort extraction. Tooling must not derive a definition from whichever runtime branch happened to execute. A production request carrying missing, unknown, or conflicting definition identity receives a contract error.

### Repeated-call behavior

`tune.number(...)` may run on every game update, but static extraction and digest calculation must not. Build tooling should generate a static descriptor for each call site. Runtime execution evaluates only bound input/context values and attaches the cached definition identity.

Rules:

- identical canonical definitions for the same decision key within one build are deduplicated,
- two call sites in one build that use the same decision key with different canonical definitions fail the build with `contract-conflict`,
- development runtime extraction may memoize by decision key, but must reject a second digest for that key,
- different deployed builds may carry different registered revisions for the same stable key,
- the server returns governed fallback for unknown or conflicting identities rather than registering runtime-dependent semantics.

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
    lifecycle: {
      authorityMode: "proposal-managed"
    },
    policy: {
      kind: "inline",
      constraints: [
        { kind: "max-delta", value: 50 },
        { kind: "cooldown", seconds: 20 },
        { kind: "min-sample-size", value: 30 },
        { kind: "min-evidence-quality", value: 0.7 },
        { kind: "max-model-uncertainty", value: 0.35 }
      ]
    },
    context: {
      sessionId: { type: "string", target: "session" },
      userId: { type: "string", target: "user" },
      cohort: { type: "string", target: "cohort" }
    }
  },
  context: {
    sessionId,
    userId,
    cohort: playerCohort
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

The generated wire model uses the shared `valueType + value` discriminated union. The ergonomic SDK may project that union into a generic only after runtime validation:

```ts
type DecisionResult<T> =
  | ServerDecisionResult<T>
  | ClientFallbackResult<T>;
```

`confidence` is present only when the returned authority makes an
evidence-backed claim. It is `null` for decision fallback and deterministic
bundle-authored strategies. Resolution fallback preserves the broader
authority's semantics: evidence-backed authority retains confidence, while a
deterministic broader-target strategy returns `null`.

`decisionMode` tells the application how the value was produced without exposing internal implementation details. For the Tetris adaptive MVP, the expected mode is usually `strategy`: the server executed an approved strategy against live runtime context and returned an immediate numeric value.

Every server `200` contains the exact accepted definition tuple with `definitionStatus.integrity = "verified"`. Unknown, retired, or conflicting identities are typed Problem Details errors and never success-shaped fallback results.

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
  confirm(
    decisionId: string,
    confirmToken: string
  ): Promise<ExposureConfirmationResult>;
}

type ServerDecisionReceipt<T extends DecisionValue = DecisionValue> = {
  source: "server";
  value: T;
  decisionId: string;
  exposure: ExposureDirective;
};

type ClientFallbackReceipt<T extends DecisionValue = DecisionValue> = {
  source: "client-fallback";
  value: T;
  expectedContract: RuntimeContractIdentity;
  reason: string;
};

type DecisionReceipt<T extends DecisionValue = DecisionValue> =
  | ServerDecisionReceipt<T>
  | ClientFallbackReceipt<T>;

type NumberTuneRequest =
  | CodeFirstNumberTuneRequest
  | ExplicitNumberTuneRequest;

type CodeFirstNumberTuneRequest = {
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: BoundInferenceDeclaration;
  intent: DecisionIntent;
  output: NumberOutputContract;
  lifecycle: AuthorityLifecycleAuthoring;
  policy: PolicyAuthoring;
  context: BoundRuntimeContext;
};

type ExplicitNumberTuneRequest = {
  definition: AdvancedNumberTuneDefinition;
  context: RuntimeContext;
  inputs?: AnyBoundSignalInput[];
};

type AdvancedNumberTuneDefinition = {
  targetHierarchy?: string[];
  signals?: DecisionSignalReferences;
  inference?: InferenceDeclaration;
  intent: DecisionIntent;
  output: NumberOutputContract;
  lifecycle: AuthorityLifecycleAuthoring;
  policy: PolicyReference | InlinePolicy;
  context: RuntimeContextSchema;
};

type AuthorityLifecycleAuthoring =
  | BundleApprovedAuthorityAuthoring
  | { authorityMode: "proposal-managed" };

type BundleApprovedAuthorityAuthoring = {
  authorityMode: "bundle-approved";
  initialAuthority: {
    controlTarget: DecisionTargetRef;
    kind: "numeric-rule";
    rule: NumericRuleAuthoring;
    rationale: string;
  };
};

type NumericRuleAuthoring = Omit<NumericRuleDeclaration, "weightedInputs"> & {
  weightedInputs: Array<
    Omit<NumericRuleInput, "signal"> & {
      signal: InferenceSignalHandle<number>;
    }
  >;
};

type PolicyAuthoring = {
  maxDelta?: number;
  cooldown?: `${number}s`;
  minSampleSize?: number;
  minEvidenceQuality?: number;
  maxModelUncertainty?: number;
  minExpectedOutcome?: number;
  paused?: boolean;
};

type NumberOutputContract = {
  default: number;
  range: [number, number];
  step?: number;
};

type RuntimeContextSchema = Record<
  string,
  { type: "string" | "number" | "boolean"; target?: string }
>;

type RuntimeContextValue = string | number | boolean | null;

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
  evidence?: SignalIdentity[];
  guardrails?: SignalIdentity[];
};

type SignalIdentity = {
  readonly key: string;
};

type SignalHandle<T> = SignalIdentity & {
  readonly schemaDigest?: string;
  emit(value: T): void;
};

type InferenceValue = boolean | number | string;

declare const metricValueType: unique symbol;
declare const inferenceMetricType: unique symbol;

type MetricIdentity<
  T extends InferenceValue = InferenceValue
> = SignalIdentity & {
  readonly [metricValueType]: T;
};

type NumericMetricIdentity = MetricIdentity<number>;

type InferenceMetricIdentity<
  T extends InferenceValue = InferenceValue
> = MetricIdentity<T> & {
  readonly [inferenceMetricType]: true;
};

type InferenceSignalHandle<T extends InferenceValue> =
  SignalHandle<T> &
  InferenceMetricIdentity<T> & {
    input(value: T): BoundSignalInput<T>;
  };

type BoundSignalInput<T extends InferenceValue> = {
  readonly signal: InferenceSignalHandle<T>;
  readonly value: T;
};

type AnyBoundSignalInput =
  | BoundSignalInput<boolean>
  | BoundSignalInput<number>
  | BoundSignalInput<string>;

type InferenceDeclaration = {
  target: "global" | "segment" | "cohort" | "user" | "session" | "level" | string;
  inputs?: InferenceMetricIdentity[];
  fallbackOrder?: string[];
};

type BoundInferenceDeclaration = {
  target: "global" | "segment" | "cohort" | "user" | "session" | "level" | string;
  inputs?: AnyBoundSignalInput[];
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

type MetricObjective =
  | {
      signal: NumericMetricIdentity;
      direction: "minimize" | "maximize";
    }
  | {
      signal: NumericMetricIdentity;
      direction: "target";
      target: number;
    };

interface IDefinitionBundleProvider {
  exportBundle(): DecisionDefinitionBundle;
  getRegistrationReceipt(): RegistrationReceipt | undefined;
}
```

Constructor generics preserve producer-side type safety:

```ts
declare const boardPressureSignal: InferenceSignalHandle<number>;
declare const earlyLossRate24hSignal: SignalHandle<number> & NumericMetricIdentity;
declare const difficultyLabelSignal: SignalHandle<string> & MetricIdentity<string>;
declare const piecePlacedEvent: SignalHandle<{
  placementTimeMs: number;
  hardDrop: boolean;
}>;

boardPressureSignal.input(0.82);
const objective: MetricObjective = {
  signal: earlyLossRate24hSignal,
  direction: "minimize"
};
piecePlacedEvent.emit({ placementTimeMs: 420, hardDrop: true });

boardPressureSignal.input("high"); // TypeScript error
piecePlacedEvent.emit({ placementTimeMs: 420, hardDrop: "yes" }); // TypeScript error
const eventObjective: MetricObjective = {
  signal: piecePlacedEvent, // TypeScript error
  direction: "minimize"
};
const stringObjective: MetricObjective = {
  signal: difficultyLabelSignal, // TypeScript error
  direction: "target",
  target: 0.5
};
const missingTarget: MetricObjective = {
  signal: earlyLossRate24hSignal,
  direction: "target" // TypeScript error
};
const unexpectedTarget: MetricObjective = {
  signal: earlyLossRate24hSignal,
  direction: "minimize",
  target: 0.5 // TypeScript error
};
```

`NumericRuleAuthoring.weightedInputs[].signal` accepts only
`InferenceSignalHandle<number>`, so boolean/string metrics, derived metrics,
and events fail at authoring time before registry validation repeats the check.

The SDK should not implement policy, strategy selection, async intelligence, or server state. Its responsibilities are definition extraction from static request fields, telemetry, optional definition bundle export, runtime request, compact definition identity propagation, typed response, and explicitly configured local fallback for data-plane availability failures.

In the basic path, `default` is the singular safe fallback value. During bundle generation, the SDK can compile it into the lower-level action-space default and fallback definition required by the registry/runtime model.

## Contract authoring, bundle, and registration

The client library supports code-first decision declarations while preserving separate management and runtime APIs. For MVP, trusted application/bootstrap startup is the default control-plane client experience.

Decision declarations should be extractable into a canonical `DecisionDefinitionBundle`. Build, release, deployment, GitOps, or operator tooling can validate and apply that bundle to Flaggo management APIs.

Recommended code-first lifecycle:

```text
write declaration in code
  -> extract flaggo.decision-definition-bundle.json
  -> deploy application independently
  -> trusted startup validates/applies bundle through management API
  -> authorized actor approves the exact initial-authority snapshot
  -> service activates state through the shared activation boundary
  -> receive ready registration receipt
  -> initialize data-plane client with accepted identity
  -> runtime decide succeeds only for exact registered identity
```

Supported authoring modes:

| Mode | Behavior |
| --- | --- |
| `code-first` | SDK declarations generate or contribute to a canonical `DecisionDefinitionBundle`. |
| `bundle-first` | Application or platform uses hand-authored JSON/YAML bundle; SDK may only reference identity. |
| `registry-first` | Decision key is managed in Flaggo registry or operator tooling; SDK references pre-registered decision definition and expected identity. |
| `local-only` | Use declarations and fallback locally without remote validation. Useful for demos and early development. |

The first slice should support TypeScript `code-first` generation plus startup registration for the local Tetris demo and direct `bundle-first` REST compatibility at the management API layer.

Resource lifecycle is owned by the Contract Registry. Client tooling should create or validate resources through bundle sync, but should not hard-delete missing resources. Missing declarations should become deprecation candidates, not deletes.

### Versioning UX

Developers should not need to manually version every decision definition during normal development. The SDK and registry can hide most version management behind validation/apply:

```text
developer names the decision: tetris.dropInterval
  -> tooling extracts canonical semantics
  -> registry compares with existing definition
  -> unchanged or metadata-only: keep the same runtime tuple
  -> semantic change: require approval, then receive a new opaque revision/digest
```

The developer-facing name can stay stable for the common case. When semantics change in a way that could make old decision state unsafe, tooling should make the change explicit in PR/CI output:

```text
tetris.dropInterval
semantic change detected: added required signal recoveryFailures
state reuse: no
telemetry reuse: boardPressure and placementTimeMs evidence can be reused
new evidence warmup: recoveryFailures
runtime revision: assigned by registry after approval
```

This keeps the normal UX simple while still preventing accidental sharing of active strategies across incompatible contracts.

## Runtime definition identity

The production runtime SDK must send compact expected definition identity:

```ts
const receipt = flaggo.definitions.getRegistrationReceipt();
const decisionKey = "tetris.dropInterval";
const acceptedDefinition =
  receipt?.acceptedDefinitions[decisionKey];

if (!acceptedDefinition) {
  throw new MissingAcceptedDefinitionError(decisionKey);
}

const decision = await dropInterval.decide({
  expectedContract: {
    definitionId: acceptedDefinition.definitionId,
    revision: acceptedDefinition.revision,
    contractDigest: acceptedDefinition.contractDigest,
    bundleDigest: receipt.bundleDigest,
    buildId: receipt.buildId
  },
  runtimeContext: {
    userId,
    sessionId,
    cohort: playerCohort
  },
  inputs: [
    boardPressureSignal.input(boardPressure),
    recentPlacementTimeMsSignal.input(recentPlacementTimeMs),
    recoveryFailuresSignal.input(recoveryFailures),
    currentLevelSignal.input(game.level)
  ]
});
```

The full `DecisionDefinitionBundle` should not be sent with each runtime request. Runtime identity can come from:

- SDK build metadata,
- generated constants,
- environment variables,
- deployment annotations or injected config,
- direct REST headers/body fields.

`runtimeContext` carries declared contextual and target-binding fields.
Strategy operands use the canonical `inputs` collection; the SDK must not move
or duplicate inference values into context.

Different builds of the same service can be deployed at the same time. The SDK should treat expected definition identity as build/deployment metadata attached to each workload.

If expected identity is missing, unknown, conflicting, or retired, the SDK surfaces the Problem Details contract error. It must not invoke local fallback or retry with another revision. Local fallback is reserved for explicitly configured data-plane availability failures.

Production credentials use OAuth 2.0/OIDC scopes. The SDK keeps management credentials out of runtime/browser clients and exposes an explicit insecure local-development bypass only when configured. Decide retries may supply an optional `Idempotency-Key`; the SDK sends tracing identity only through `X-Flaggo-Correlation-Id` and never uses it as retry identity. Detailed results expose server-resolved target provenance so callers can distinguish verified, derived, and replaced cohort claims.

The exact availability classifier and retry defaults are defined in the [API Contract Proposal](../API_CONTRACT_PROPOSAL.md#sdk-availability-fallback-classifier). The SDK must compare each generated call-site digest with `RegistrationReceipt.acceptedDefinitions[decisionKey]` before a remote attempt or local fallback. Availability fallback is forbidden until that accepted binding exists and matches.

For valid Flaggo Problem Details, the SDK requires `clientFallback.eligible: true`; status `503` alone does not authorize a local value. In particular, `required-evidence-unavailable` is forbidden unless the registered policy separately permits it and the server projects that permission into the error.

## Telemetry behavior

The client library should expose a domain-event API, but the transport should prefer OpenTelemetry.

Initial telemetry modes:

| Mode | Behavior |
|---|---|
| `opentelemetry` | Emit via OTel-compatible exporter/collector. |
| `direct` | Send to a Flaggo development/demo ingestion endpoint. |
| `disabled` | Do not emit telemetry; decisions rely on existing server evidence or fallback. |

Domain events should be developer-friendly. The SDK may map them to structured logs, span events, or metric measurements under the hood.

## Accepted wire-contract decisions

The [API Contract Proposal decision log](../API_CONTRACT_PROPOSAL.md#contract-decision-log) records the accepted decisions. The client library requires exact runtime identity (A1), keeps server and client fallback result types distinct (A2), confirms exposure with an opaque token (A4), treats cohort/segment IDs as verifiable claims (A8), uses compact runtime evidence/provenance (A9), omits batch decide in Phase 1 (A10), uses scoped OAuth 2.0/OIDC credentials in production (A11), and supports optional decide idempotency keys (A12).

Client-only questions that remain:

- How much local metric computation should browser SDKs perform before normal telemetry emission?
- Should direct telemetry mode exist only in local/demo environments?
- Which generator produces TypeScript wire types from OpenAPI without replacing ergonomic SDK/domain types?

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
- Trusted startup registration through the control-plane validate/apply API.
- Expected definition digest/revision propagation.
- Decision API call with a typed response.
- Runtime API path versioning through `/v1`.
- OpenTelemetry telemetry mode.
- No registration side effects in decide or exposure calls.
