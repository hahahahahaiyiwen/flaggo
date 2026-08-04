import { describe, expectTypeOf, it } from "vitest";

import type {
  ServerDecisionResult,
  StrategyDecisionResult,
} from "../src/index.js";

describe("runtime result discriminated unions", () => {
  it("correlates numeric values and decision modes", () => {
    const inspect = (result: ServerDecisionResult<number>): void => {
      expectTypeOf(result.value).toEqualTypeOf<number>();
      expectTypeOf(result.valueType).toEqualTypeOf<"number">();
      if (result.decisionMode === "fallback") {
        expectTypeOf(result.confidence).toEqualTypeOf<null>();
        expectTypeOf(result.fallback.decisionFallbackUsed)
          .toEqualTypeOf<true>();
        expectTypeOf(result.policy.result)
          .toEqualTypeOf<"blocked" | "fallback">();
      } else if (
        result.decisionMode === "strategy"
        || result.decisionMode === "experiment"
      ) {
        expectTypeOf(result.strategyId).toEqualTypeOf<string>();
        expectTypeOf(result.fallback.decisionFallbackUsed)
          .toEqualTypeOf<false>();
      }
      result.policy.clientFallback?.requiredEvidenceUnavailable;
    };
    void inspect;
  });

  it("rejects impossible strategy results", () => {
    // @ts-expect-error strategies require strategyId and non-null confidence
    const invalid: StrategyDecisionResult<number> = {
      source: "server",
      decisionMode: "strategy",
      valueType: "number",
      value: 700,
    };
    void invalid;
  });
});
