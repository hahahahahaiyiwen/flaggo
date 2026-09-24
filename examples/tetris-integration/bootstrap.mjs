import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { publishLocalManifest } from "../shared/publish-local-manifest.mjs";
import {
  publishJsonGeneration,
  writeJsonAtomic,
} from "./durable-json.mjs";

export {
  publishJsonArtifact,
  publishJsonGeneration,
  resolveJsonGeneration,
  writeJsonAtomic,
} from "./durable-json.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
export const canonicalBundlePath = resolve(
  exampleDirectory,
  "tetris-definition-bundle.json",
);
export const strategyActivationPath = resolve(
  exampleDirectory,
  "strategy-activation.json",
);
export async function loadCanonicalBundle(signal) {
  return JSON.parse(await readFile(canonicalBundlePath, { encoding: "utf8", signal }));
}

export async function createActivatedState(
  receipt,
  now = new Date(),
  signal,
) {
  const activation = JSON.parse(
    await readFile(strategyActivationPath, { encoding: "utf8", signal }),
  );
  const accepted = receipt.acceptedDefinitions[activation.decisionKey];
  if (accepted === undefined) {
    throw new Error(
      `Registration receipt does not accept '${activation.decisionKey}'.`,
    );
  }
  const changedAt = new Date(
    now.getTime() + activation.lastChangedAtOffsetSeconds * 1000,
  ).toISOString();
  return {
    version: 1,
    states: [
      {
        decisionKey: activation.decisionKey,
        definitionId: accepted.definitionId,
        revision: accepted.revision,
        contractDigest: accepted.contractDigest,
        value: activation.value,
        controlTarget: activation.controlTarget,
        mode: activation.mode,
        strategyId: activation.strategyId,
        numericRule: activation.numericRule,
        lastChangedAt: changedAt,
      },
    ],
  };
}

export async function bootstrapTetris({
  controlPlaneUrl,
  publicationPath,
  fetchImpl = globalThis.fetch,
  now = new Date(),
  signal,
  publishGeneration = publishJsonGeneration,
}) {
  signal?.throwIfAborted();
  const bundle = await loadCanonicalBundle(signal);
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
  signal?.throwIfAborted();
  const state = await createActivatedState(receipt, now, signal);
  const evidence = { version: 1, evidenceByStrategy: {} };
  const publication = await publishGeneration(
    publicationPath,
    { evidence, receipt, state },
    signal,
  );
  signal?.throwIfAborted();
  return {
    ...publicationResult,
    receipt,
    state,
    evidence,
    publication,
  };
}

function options(args) {
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error("Bootstrap arguments must be --name value pairs.");
    }
    values.set(key.slice(2), value);
  }
  return values;
}

async function main() {
  const values = options(process.argv.slice(2));
  const publicationPath = values.get("output");
  if (publicationPath === undefined) {
    throw new Error(
      "Usage: node bootstrap.mjs --control-plane URL --output PATH",
    );
  }
  const result = await bootstrapTetris({
    controlPlaneUrl: values.get("control-plane") ?? "http://127.0.0.1:5081",
    publicationPath,
  });
  process.stdout.write(`${JSON.stringify({
    status: "ready",
    approvalRequired: result.approvalRequired,
    approvalRequestId: result.approvalRequestId,
    bundleDigest: result.receipt.bundleDigest,
    revision:
      result.receipt.acceptedDefinitions["tetris.dropInterval"].revision,
    publicationPath: result.publication.rootPath,
    generation: result.publication.generation,
    statePath: result.publication.paths.state,
    receiptPath: result.publication.paths.receipt,
    evidencePath: result.publication.paths.evidence,
  }, null, 2)}\n`);
}

if (
  process.argv[1] !== undefined
  && pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
