import { expectTypeOf, it } from "vitest";
import { createFlaggoClient, type DecisionReceipt, type RegistrationReceipt, type RuntimeCatalog } from "../src/index.js";

const digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const catalog = {
  format: "flaggo.runtime-catalog/v1", application: { id: "app", environment: "test" }, bundleDigest: digest,
  decisions: {
    live: {
      result: { type: "number", min: 0, max: 1, default: 0 },
      context: { sessionId: { type: "string", required: true } },
      inputs: { pressure: { source: "request", type: "number", meaning: "Live occupancy." } }, contractDigest: digest,
    },
    fixed: { result: { type: "number", min: 0, max: 1, default: 0 }, context: {}, inputs: {}, contractDigest: digest },
    materialized: {
      result: { type: "number", min: 0, max: 1, default: 0 }, context: {},
      inputs: { pressure: { source: "evidence", binding: "latest" } }, contractDigest: digest,
    },
    enabled: { result: { type: "boolean", default: false }, context: {}, inputs: {}, contractDigest: digest },
  },
} as const satisfies RuntimeCatalog;

it("derives keys, result kinds, context requiredness and caller-owned inputs from a generated catalog", () => {
  const inspect = (receipt: RegistrationReceipt): void => {
    const client = createFlaggoClient({ catalog, receipt, dataPlaneUrl: "https://data.test" });
    expectTypeOf(client.tune.number("fixed")).toEqualTypeOf<Promise<DecisionReceipt<number>>>();
    client.tune.number("materialized");
    client.tune.number("live", { context: { sessionId: "session-1" }, inputs: { pressure: 0.5 } });
    // @ts-expect-error unknown key
    client.tune.number("unknown");
    // @ts-expect-error wrong result kind
    client.tune.number("enabled");
    // @ts-expect-error missing required runtime data
    client.tune.number("live");
    // @ts-expect-error missing required context
    client.tune.number("live", { inputs: { pressure: 0.5 } });
    // @ts-expect-error wrong primitive type
    client.tune.number("live", { context: { sessionId: "session-1" }, inputs: { pressure: "high" } });
    // @ts-expect-error evidence-owned values cannot be supplied by callers
    client.tune.number("materialized", { inputs: { pressure: 0.5 } });
    // @ts-expect-error no invented inputs for key-only definitions
    client.tune.number("fixed", { inputs: { pressure: 0.5 } });
    const key = Math.random() < 0.5 ? "fixed" : "live";
    // @ts-expect-error a union key cannot erase required caller data
    client.tune.number(key);
    // @ts-expect-error detailed calls preserve the same key/request correlation
    client.tune.numberDetailed(key);
    // @ts-expect-error live data is invalid when the dynamic key selects fixed
    client.tune.number(key, { context: { sessionId: "session-1" }, inputs: { pressure: 0.5 } });
    if (key === "live") {
      client.tune.number(key, { context: { sessionId: "session-1" }, inputs: { pressure: 0.5 } });
      client.tune.numberDetailed(key, { context: { sessionId: "session-1" }, inputs: { pressure: 0.5 } });
    }
    const call = Math.random() < 0.5
      ? ["fixed"] as const
      : ["live", { context: { sessionId: "session-1" }, inputs: { pressure: 0.5 } }] as const;
    client.tune.number(...call);
    client.tune.numberDetailed(...call);
  };
  void inspect;
});
