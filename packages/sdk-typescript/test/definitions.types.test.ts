import { describe, expectTypeOf, it } from "vitest";

import type {
  BooleanDecisionDefinition,
  DecisionDefinition,
  InlinePolicy,
  ReferencePolicy,
  StringDecisionDefinition,
} from "../src/index.js";

describe("definition contract unions", () => {
  it("supports boolean and string definitions", () => {
    expectTypeOf<BooleanDecisionDefinition>()
      .toMatchTypeOf<DecisionDefinition>();
    expectTypeOf<StringDecisionDefinition>()
      .toMatchTypeOf<DecisionDefinition>();
  });

  it("requires policy discriminator fields", () => {
    // @ts-expect-error reference policies require policyId
    const invalidReference: ReferencePolicy = { kind: "reference" };
    // @ts-expect-error inline policies require constraints
    const invalidInline: InlinePolicy = { kind: "inline" };
    void [invalidReference, invalidInline];
  });

  it("correlates action spaces and fallback values", () => {
    const invalid: BooleanDecisionDefinition = {
      key: "feature.enabled",
      valueType: "boolean",
      actionSpace: { type: "boolean", default: false },
      fallback: {
        // @ts-expect-error boolean definitions require boolean fallback values
        value: "false",
      },
      intent: {},
      policy: { kind: "inline", constraints: [] },
    };
    void invalid;
  });
});
