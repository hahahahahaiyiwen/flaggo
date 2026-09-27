import { expectTypeOf, it } from "vitest";

import type {
  ProblemDetails,
  RuntimeDecision,
} from "../src/runtime/index.js";

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

it("models RFC 9457 standard members as optional and permits extensions", () => {
  const inspect = (problem: ProblemDetails): void => {
    expectTypeOf(problem.type).toEqualTypeOf<string | undefined>();
    expectTypeOf(problem.status).toEqualTypeOf<number | undefined>();
    expectTypeOf(problem.traceId).toEqualTypeOf<unknown>();
  };
  void inspect;
});
