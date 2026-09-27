# `@flaggo/sdk`

The TypeScript SDK calls the stateless Decision Service v3 endpoint:

```http
POST /v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions
```

Bind each decision name to one immutable contract version when creating the
client:

```ts
import {
  createFlaggoClient,
  type RuntimeContractBindings,
} from "@flaggo/sdk";

const contracts = {
  "worker.batch-size": {
    contractDigest:
      "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  },
} as const satisfies RuntimeContractBindings;

const flaggo = createFlaggoClient({
  decisionServiceUrl: "https://decisions.example.com",
  contracts,
  credential: {
    mode: "bearer",
    getToken: async () => obtainAccessToken(),
  },
  retries: 1,
});

const decision = await flaggo.decide<number>("worker.batch-size", {
  attributes: {
    queuePressure: 0.82,
    workerId: "worker-17",
  },
  currentExposure: {
    exposureId: "previous-exposure-id",
  },
});

applyBatchSize(decision.result);
```

The SDK builds the complete `RuntimeInput`, adds the reserved `_random`
attribute, and serializes it once. A retry reuses those exact bytes, while the
Decision Service intentionally resolves whichever executable is active for
that later attempt. Application attributes beginning with `_` are rejected.

Successful responses are strict `RuntimeDecision` values containing
`contractDigest`, `executableDigest`, `result`, and rule/default evaluation
provenance. Failures use standard RFC 9457 Problem Details; correlation and
retry metadata are read from HTTP headers. The v3 client does not implement
local fallback because fallback behavior remains a separate design decision.

## Contract management

`createContractServiceClient` exposes the five Management API v3 operations:
dry-run validation, idempotent publication by contract name, current-version
lookup, paginated version history, and exact-version lookup.

```ts
import {
  createContractServiceClient,
  type DecisionContract,
} from "@flaggo/sdk";

const contracts = createContractServiceClient({
  contractServiceUrl: "https://contracts.example.com",
  credential: {
    mode: "bearer",
    getToken: async () => obtainAccessToken(),
  },
});

const contract: DecisionContract = {
  // Complete DecisionContract payload.
};

const validation = await contracts.validate("worker.batch-size", contract);
if (validation.status === "valid") {
  const accepted = await contracts.put("worker.batch-size", contract);
  const exact = await contracts.getVersion(
    accepted.name,
    accepted.contractDigest,
  );
}
```

The name identifies the logical contract resource and `contractDigest`
identifies one immutable version. `getCurrent` is a management projection only;
runtime clients must use the exact digest returned by publication or lookup.

## Validation

```powershell
npm test --workspace @flaggo/sdk
npm run build --workspace @flaggo/sdk
```
