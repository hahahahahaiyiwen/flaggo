import { resolve } from "node:path";

import { deployContracts } from "../contract-deployment.mjs";

export async function deployTetrisContract({
  exampleDirectory,
  contractUrl,
  fetch,
  signal,
}) {
  const result = await deployContracts({
    manifestPath: resolve(exampleDirectory, "flaggo.deploy.json"),
    baseUrl: contractUrl,
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
  return deployed;
}
