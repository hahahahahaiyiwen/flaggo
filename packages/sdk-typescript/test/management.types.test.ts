import { expectTypeOf, it } from "vitest";

import {
  defineDecisionContract,
  type ContractClient,
  type DecisionContractVersion,
  type FlaggoResponse,
} from "../src/management/index.js";

it("infers deployment result types from the submitted contract", () => {
  const contract = defineDecisionContract<number>({
    name: "worker.batchSize",
    expression_syntax: "flaggo.cel/v1",
    attributes: [
      {
        name: "pressure",
        schema: { type: "number", minimum: 0, maximum: 1 },
      },
    ],
    result: {
      schema: { type: "integer", minimum: 1 },
      default: 3,
    },
  });
  const client = {} as ContractClient;

  if (false) {
    expectTypeOf(client.deploy(contract)).toEqualTypeOf<
      Promise<FlaggoResponse<DecisionContractVersion<number>>>
    >();
  }
});
