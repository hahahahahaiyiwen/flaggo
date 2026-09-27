import { expectTypeOf, it } from "vitest";

import {
  createFlaggoClient,
  type JsonValue,
  type RuntimeContractBindings,
  type RuntimeDecision,
} from "../src/index.js";

const digest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const contracts = {
  parallelism: { contractDigest: digest },
  enabled: { contractDigest: digest },
} as const satisfies RuntimeContractBindings;

it("preserves exact contract names and caller-selected result types", () => {
  const client = createFlaggoClient({
    contracts,
    decisionServiceUrl: "https://decisions.test",
  });

  expectTypeOf(client.decide).toBeFunction();
  if (false) {
    expectTypeOf(client.decide("parallelism"))
      .toEqualTypeOf<Promise<RuntimeDecision<JsonValue>>>();
    expectTypeOf(client.decide<number>("parallelism"))
      .toEqualTypeOf<Promise<RuntimeDecision<number>>>();
    expectTypeOf(client.decide<boolean>("enabled"))
      .toEqualTypeOf<Promise<RuntimeDecision<boolean>>>();
    // @ts-expect-error unknown contract names are rejected by generated bindings
    void client.decide("unknown");
  }
});
