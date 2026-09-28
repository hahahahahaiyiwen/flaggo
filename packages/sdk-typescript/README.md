# `@flaggo/sdk`

The TypeScript SDK provides separate clients for runtime decisions and contract
management:

```ts
import { createDecisionClient } from "@flaggo/sdk/runtime";
import { createContractClient } from "@flaggo/sdk/management";
```

The package is ESM-only and targets Node.js 20+ with the standard Fetch API.

## Runtime decisions

Application configuration binds every decision name to one immutable accepted
contract digest. A TypeScript decision catalog binds that identity to its
attribute and result types once, rather than selecting a result type at each
call.

```ts
import {
  createDecisionClient,
  defineDecisionBindings,
  type DecisionSpec,
} from "@flaggo/sdk/runtime";

type Decisions = {
  readonly "worker.batch-size": DecisionSpec<{
    readonly queuePressure: number;
    readonly workerId: string;
  }, number>;
};

const bindings = defineDecisionBindings<Decisions>({
  "worker.batch-size": {
    contractDigest:
      "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  },
});

const flaggo = createDecisionClient<Decisions>({
  baseUrl: "https://decisions.example.com",
  bindings,
  credential: {
    mode: "bearer",
    getToken: async () => obtainAccessToken(),
  },
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

After a successful response, `decide()` emits a local OpenTelemetry log event
named `flaggo.decision.received` through `@opentelemetry/api-logs`. That event
is a raw decision observation, not proof that the app applied the result and
not clean learning evidence by itself. If no OpenTelemetry logger provider is
configured, the API behaves as a no-op.

Applications can report potentially relevant outcomes with SDK helpers when
convenient:

```ts
import { createFlaggoTelemetry } from "@flaggo/sdk/runtime";

const telemetry = createFlaggoTelemetry();

telemetry.recordOutcome({
  binding: "worker.latency_ms",
  value: 125,
  decisionId: "optional-decision-id",
  contractName: "worker.batch-size",
  contractDigest:
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  correlation: {
    workerId: "worker-17",
  },
});
```

Apps may also emit ordinary OpenTelemetry telemetry without the helper. The
Flaggo Collector/Ingestion profile treats SDK helpers as convenience APIs, not
the only valid source of outcome observations.

## Contract management

The management client exposes dry-run validation, idempotent deployment,
current-version lookup, paginated history, and exact-version lookup.

```ts
import {
  createContractClient,
  type DecisionContract,
} from "@flaggo/sdk/management";

const contracts = createContractClient({
  baseUrl: "https://contracts.example.com",
  credential: {
    mode: "bearer",
    getToken: async () => obtainAccessToken(),
  },
});

const contract = {
  name: "worker.batch-size",
  expression_syntax: "flaggo.cel/v1",
  attributes: [],
  result: {
    schema: { type: "integer", minimum: 1 },
    default: 3,
  },
} as const satisfies DecisionContract<number>;

const deployed = await contracts.deploy(contract);
const exact = await contracts.getVersion(
  deployed.value.name,
  deployed.value.contractDigest,
);
console.log(exact.value.contract);
```

`deploy` performs local wire-shape validation before sending the authoritative
`PUT`. Use `validate` separately when an authoring or CI workflow needs
server-side semantic diagnostics without mutation; it is not a prerequisite
for deployment.

The server computes contract and executable digests. `getCurrent` is a
management projection only; runtime clients always use an exact digest.

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

Wire models and standalone validators are generated from the v3 JSON Schemas.
Client orchestration, retries, exact-version binding, and semantic identity
checks remain hand-written.

```powershell
npm run check:generated --workspace @flaggo/sdk
npm run typecheck --workspace @flaggo/sdk
npm test --workspace @flaggo/sdk
npm run build --workspace @flaggo/sdk
```
