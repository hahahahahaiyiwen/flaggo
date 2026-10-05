# `@flaggo/sdk`

The TypeScript SDK provides separate clients for runtime decisions and contract
management plus strict parsers for generated application configuration:

```ts
import {
  parseFlaggoDeploymentManifest,
  parseFlaggoRuntimeConfiguration,
  parseFlaggoServiceEndpoints,
} from "@flaggo/sdk/configuration";
import { createDecisionClient } from "@flaggo/sdk/runtime";
import { createContractClient } from "@flaggo/sdk/management";
```

The package is ESM-only and targets Node.js 20+ with the standard Fetch API.
Deployment tooling can call `parseFlaggoServiceEndpoints` before performing
any service mutation. The same bounded HTTP(S) endpoint validation is applied
when runtime configuration is parsed and when SDK transports are created.

## Runtime decisions

Application configuration binds every decision name to one immutable accepted
contract digest. A TypeScript decision catalog binds that identity to its
attribute and result types once, rather than selecting a result type at each
call.

```ts
import {
  createDecisionClient,
  type DecisionBindings,
  type DecisionSpec,
} from "@flaggo/sdk/runtime";
import {
  parseFlaggoRuntimeConfiguration,
} from "@flaggo/sdk/configuration";

type Decisions = {
  readonly "worker.batch-size": DecisionSpec<{
    readonly queuePressure: number;
    readonly workerId: string;
  }, number>;
};

const runtimeConfig = parseFlaggoRuntimeConfiguration<
  DecisionBindings<Decisions>
>(generatedRuntimeConfiguration);

const flaggo = createDecisionClient<Decisions>({
  runtimeConfig,
  retry: {
    maxAttempts: 2,
  },
});

const { value: decision, metadata } = await flaggo.decide(
  "worker.batch-size",
  {
    attributes: {
      queuePressure: 0.82,
      workerId: "worker-17",
    },
    currentExposure: {
      exposureId: "previous-exposure-id",
    },
  },
  {
    correlationId: "request-correlation-id",
    timeoutMs: 1_000,
    signal: abortController.signal,
  },
);

applyBatchSize(decision.result);
console.log(metadata.correlationId);
```

The SDK constructs the complete `RuntimeInput`, adds `_random`, validates the
serialized JSON, and sends:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

All attributes are optional at evaluation time. Rules depending on missing
attributes do not match, allowing the Decision Service to return the contract
default. Transport retries reuse the exact serialized input while resolving
the executable active for each later attempt.

After a successful response, `decide()` emits a Flaggo OTLP logs mapping event
named `flaggo.decision.received` through `@opentelemetry/api-logs`. That event
is a raw decision observation, not proof that the app applied the result and
not clean learning evidence by itself. If no OpenTelemetry logger provider is
configured, the API behaves as a no-op.

Applications emit potentially relevant outcomes through their normal
OpenTelemetry logs, metrics, spans, and span events. Each learning evidence
declaration identifies exactly one application signal source. The receiver
durably enqueues complete valid export requests without extracting authority or
routing metadata. The asynchronous materializer derives
`AuthorityScope + SourceKey`, checks active route reference counts, and stores
each shared observation once. Exact contract association, predicates, and
correlation mappings are evaluated during analysis.

`flaggo.decision.received` remains a built-in protocol observation that the
materializer always recognizes. Its digest identifies the decision that
occurred; ordinary application evidence carries no contract digest.

Applications can add Flaggo as a direct OTLP/HTTP JSON destination through the
optional OpenTelemetry integration entry:

```ts
import { LoggerProvider } from "@opentelemetry/sdk-logs";
import { MeterProvider } from "@opentelemetry/sdk-metrics";
import { TracerProvider } from "@opentelemetry/sdk-trace";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  createFlaggoResource,
  createFlaggoLogRecordProcessor,
  createFlaggoMetricReader,
  createFlaggoSpanProcessor,
} from "@flaggo/sdk/opentelemetry";

const resource = createFlaggoResource({
  baseResource: resourceFromAttributes({
    "service.name": "adaptive-worker",
  }),
  runtimeConfig,
});
const loggerProvider = new LoggerProvider({
  resource,
  processors: [
    existingVendorLogProcessor,
    createFlaggoLogRecordProcessor({ runtimeConfig }),
  ],
});
const meterProvider = new MeterProvider({
  resource,
  readers: [
    existingVendorMetricReader,
    createFlaggoMetricReader({ runtimeConfig }),
  ],
});
const tracerProvider = new TracerProvider({
  resource,
  spanProcessors: [
    existingVendorSpanProcessor,
    createFlaggoSpanProcessor({ runtimeConfig }),
  ],
});

const flaggo = createDecisionClient({
  runtimeConfig,
  telemetry: {
    logger: loggerProvider.getLogger("@flaggo/sdk"),
  },
});
```

The application retains ownership of its providers, base resource, existing
vendor exporters, force-flush behavior, and shutdown. `createFlaggoResource`
adds `flaggo.tenant`, `flaggo.application`, and `flaggo.environment` from the
generated configuration and rejects conflicting values already present on the
base Resource. Export helpers reject records whose Resource does not match that
same authority instead of silently rewriting them.

Flaggo helpers return standard batched log/span processors and a periodic
metric reader; they do not create or register providers. Optional
`shouldExport` selectors reduce transport volume only and remove discarded
telemetry from Flaggo's replay window. When a selector is omitted, the helper
exports all telemetry it receives.

Lower-level `createFlaggoLogExporter`, `createFlaggoMetricExporter`, and
`createFlaggoTraceExporter` factories are available when an application needs
to compose different standard processors or readers.

## Contract management

The management client exposes dry-run validation, idempotent deployment,
current-version lookup, paginated history, and exact-version lookup.

```ts
import {
  bindDecisionContract,
  createContractClient,
  defineDecisionContract,
} from "@flaggo/sdk/management";

const contracts = createContractClient({
  baseUrl: "https://contracts.example.com",
});

const definition = defineDecisionContract({
  name: "worker.batch-size",
  expression_syntax: "flaggo.cel/v1",
  attributes: [],
  result: {
    schema: { type: "integer", minimum: 1 },
    default: 3,
  },
});
const contract = bindDecisionContract(definition, {
  tenant: "acme",
  application: "worker",
  environment: "production",
});

const deployed = await contracts.deploy(contract);
const exact = await contracts.getVersion(
  deployed.value.name,
  deployed.value.contractDigest,
);
console.log(exact.value.contract);
```

`bindDecisionContract` represents the deployment boundary: deployment tooling
uses the manifest authority to produce the complete scoped contract before
validation or deployment. `deploy` performs local wire-shape validation before
sending the authoritative `PUT`. Use `validate` separately when an authoring or
CI workflow needs server-side semantic diagnostics without mutation; it is not
a prerequisite for deployment.

The server computes the contract digest over authority and semantic content,
then computes the executable digest. `getCurrent` is a management projection
only; runtime clients always use an exact digest.

## Errors and response metadata

Successful operations return `{ value, metadata }`. Metadata contains the HTTP
status and any correlation, retry, or location headers supplied by the service.

All SDK failures derive from `FlaggoError`. Public categories distinguish
invalid input, missing bindings, transport failure, timeout, cancellation,
HTTP Problem Details, and malformed server responses.

`FlaggoHttpError.problem` preserves the RFC 9457 response object: standard
members are optional and problem-type extensions are retained. The actual HTTP
status and response headers remain available through `FlaggoHttpError.response`.
Failures while reading a response stream are reported as
`FlaggoTransportError`.

## Contract authority

Wire models and standalone validators are generated from the API schemas plus
`deployment-models-v2.schema.json`. `flaggo.deploy/v2` is the authored source
for one authority and a contract set. Deployment tooling produces the strict
`flaggo.runtime-config/v1` value consumed by runtime and OpenTelemetry helpers;
the parser returns a deeply frozen copy. Client orchestration, retries,
exact-version binding, and semantic identity checks remain hand-written.

```powershell
npm run check:generated --workspace @flaggo/sdk
npm run typecheck --workspace @flaggo/sdk
npm test --workspace @flaggo/sdk
npm run build --workspace @flaggo/sdk
```
