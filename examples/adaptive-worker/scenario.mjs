import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { publishLocalManifest } from "../shared/publish-local-manifest.mjs";
import { publishJsonGeneration } from "../tetris-integration/bootstrap.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
export const manifestBundlePath = resolve(
  exampleDirectory,
  "generated/definitions.json",
);
export const decisionKey = "demo.workerBatchSize";
export const strategyId = "strategy-adaptive-worker-pressure-v1";

export async function loadManifestBundle(signal) {
  return JSON.parse(
    await readFile(manifestBundlePath, { encoding: "utf8", signal }),
  );
}

export function createAdaptiveWorkerState(
  receipt,
  {
    now = new Date(),
    lastChangedAt = new Date(now.getTime() - 2000),
  } = {},
) {
  const accepted = receipt.acceptedDefinitions[decisionKey];
  if (accepted === undefined) {
    throw new Error(
      `Registration receipt does not accept '${decisionKey}'.`,
    );
  }

  return {
    version: 1,
    states: [
      {
        decisionKey,
        definitionId: accepted.definitionId,
        revision: accepted.revision,
        contractDigest: accepted.contractDigest,
        value: 3,
        controlTarget: {
          type: "cohort",
          id: "adaptive-workers",
        },
        mode: "strategy",
        strategyId,
        numericRule: {
          inputKey: "queuePressure",
          threshold: 0.7,
          valueAtOrAbove: 6,
          valueBelow: 3,
        },
        lastChangedAt: lastChangedAt.toISOString(),
      },
    ],
  };
}

export async function bootstrapAdaptiveWorker({
  controlPlaneUrl,
  publicationPath,
  fetchImpl = globalThis.fetch,
  now = new Date(),
  signal,
  publishGeneration = publishJsonGeneration,
}) {
  signal?.throwIfAborted();
  const bundle = await loadManifestBundle(signal);
  const fetchWithAbort = (input, init = {}) =>
    fetchImpl(
      input,
      signal === undefined ? init : { ...init, signal },
    );
  const publicationResult = await publishLocalManifest({
    controlPlaneUrl,
    bundle,
    fetch: fetchWithAbort,
  });
  const { receipt } = publicationResult;
  const state = createAdaptiveWorkerState(receipt, { now });
  const evidence = { version: 1, evidenceByStrategy: {} };
  const publication = await publishGeneration(
    publicationPath,
    { evidence, receipt, state },
    signal,
  );
  return {
    ...publicationResult,
    bundle,
    receipt,
    state,
    evidence,
    publication,
  };
}
