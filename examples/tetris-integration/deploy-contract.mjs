import { resolve } from "node:path";

import { deployContracts } from "../contract-deployment.mjs";

export async function deployTetrisContract({
  exampleDirectory,
  services,
  fetch,
  signal,
}) {
  const result = await deployContracts({
    manifestPath: resolve(exampleDirectory, "flaggo.deploy.json"),
    services,
    credential: { mode: "local-development" },
    fetch,
    signal,
  });
  const deployed = result.contracts[0];
  if (deployed === undefined || result.contracts.length !== 1) {
    throw new Error(
      "The Tetris deployment manifest must contain exactly one DecisionContract.",
    );
  }
  return {
    ...deployed,
    runtimeConfig: result.runtimeConfig,
  };
}
