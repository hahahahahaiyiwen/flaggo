import { expectTypeOf, it } from "vitest";

import {
  createDecisionClient,
  defineDecisionBindings,
  type DecisionSpec,
  type FlaggoResponse,
  type RuntimeDecision,
} from "../src/runtime/index.js";

const digest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";

type Decisions = {
  readonly parallelism: DecisionSpec<{
    readonly queuePressure: number;
    readonly workerId: string;
  }, number>;
  readonly enabled: DecisionSpec<{
    readonly tenant: string;
  }, boolean>;
};

const bindings = defineDecisionBindings<Decisions>({
  parallelism: { contractDigest: digest },
  enabled: { contractDigest: digest },
});

it("binds decision names, attributes, and results once at client creation", () => {
  const client = createDecisionClient<Decisions>({
    bindings,
    baseUrl: "https://decisions.test",
  });

  expectTypeOf(client.decide).toBeFunction();
  if (false) {
    expectTypeOf(client.decide("parallelism"))
      .toEqualTypeOf<Promise<FlaggoResponse<RuntimeDecision<number>>>>();
    expectTypeOf(client.decide("enabled", {
      attributes: { tenant: "tenant-a" },
    })).toEqualTypeOf<Promise<FlaggoResponse<RuntimeDecision<boolean>>>>();

    void client.decide("parallelism", {
      attributes: { queuePressure: 0.8 },
    });

    // @ts-expect-error unknown decision names are rejected by the catalog
    void client.decide("unknown");
    void client.decide("parallelism", {
      attributes: {
        // @ts-expect-error attribute values are checked against the decision spec
        queuePressure: "high",
      },
    });
    void client.decide("enabled", {
      attributes: {
        tenant: "tenant-a",
        // @ts-expect-error undeclared attributes are rejected
        pressure: 1,
      },
    });
    // @ts-expect-error callers cannot select a result type per decision call
    void client.decide<number>("parallelism");
  }
});

it("requires one exact-version binding for every catalog member", () => {
  if (false) {
    // @ts-expect-error enabled is required by the Decisions catalog
    defineDecisionBindings<Decisions>({
      parallelism: { contractDigest: digest },
    });
  }
});
