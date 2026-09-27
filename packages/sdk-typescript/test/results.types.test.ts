import { expectTypeOf, it } from "vitest";

import type { RuntimeDecision } from "../src/runtime/index.js";

it("models rule and default evaluation as a discriminated union", () => {
  const inspect = (decision: RuntimeDecision<number>): void => {
    expectTypeOf(decision.result).toEqualTypeOf<number>();
    if (decision.evaluation.source === "rule") {
      expectTypeOf(decision.evaluation.rule).toEqualTypeOf<string>();
    } else {
      expectTypeOf(decision.evaluation)
        .toEqualTypeOf<Readonly<{ source: "default" }>>();
    }
  };
  void inspect;
});
