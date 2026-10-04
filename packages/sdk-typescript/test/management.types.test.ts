import { expectTypeOf, it } from "vitest";

import {
  bindDecisionContract,
  defineDecisionContract,
  type ContractClient,
  type DecisionContractVersion,
  type FlaggoResponse,
} from "../src/management/index.js";

it("infers deployment result types from the submitted contract", () => {
  const definition = defineDecisionContract<number>({
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
  const contract = bindDecisionContract(definition, {
    tenant: "local",
    application: "worker",
    environment: "test",
  });
  const client = {} as ContractClient;

  expectTypeOf(definition).not.toHaveProperty("authority");
  expectTypeOf(contract.authority.tenant).toEqualTypeOf<string>();
  if (false) {
    expectTypeOf(client.deploy(contract)).toEqualTypeOf<
      Promise<FlaggoResponse<DecisionContractVersion<number>>>
    >();
  }
});
