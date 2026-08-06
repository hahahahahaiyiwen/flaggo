import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  RequiresApprovalError,
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";
import {
  publishJsonGeneration,
  writeJsonAtomic,
} from "./durable-json.mjs";

export {
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
export const canonicalEvidencePath = resolve(
  exampleDirectory,
  "evidence.json",
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
  dataPlaneUrl = "http://127.0.0.1:0",
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
  const config = {
    appId: bundle.application.id,
    environment: bundle.application.environment,
    dataPlaneUrl,
    controlPlane: {
      mode: "startup-register",
      url: controlPlaneUrl,
      bundle,
      credential: { mode: "local-development" },
    },
    dataPlaneCredential: { mode: "local-development" },
    fetch: fetchWithAbort,
  };
  let approvalRequired = false;
  let approvalRequestId;
  let client;
  try {
    client = await createFlaggoClient(config);
  } catch (error) {
    if (!(error instanceof RequiresApprovalError)) {
      throw error;
    }
    approvalRequired = true;
    approvalRequestId = error.approvalRequestId;
    const response = await fetchWithAbort(
      `${controlPlaneUrl.replace(/\/$/, "")}/v1/definition-bundle-approvals/${encodeURIComponent(error.approvalRequestId)}:approve`,
      {
        method: "POST",
        headers: {
          Authorization: "Flaggo-Local-Development",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          expectedBundleDigest: error.bundleDigest,
          comment: "Trusted Phase 3 Tetris local bootstrap.",
        }),
      },
    );
    const approval = await response.json();
    if (!response.ok || approval.status !== "approved") {
      throw new Error(
        `Tetris definition approval failed with HTTP ${response.status}: ${JSON.stringify(approval)}`,
      );
    }
    client = await createFlaggoClient(config);
  }

  const receipt = client.definitions.getRegistrationReceipt();
  signal?.throwIfAborted();
  const state = await createActivatedState(receipt, now, signal);
  const evidence = JSON.parse(
    await readFile(canonicalEvidencePath, { encoding: "utf8", signal }),
  );
  const publication = await publishGeneration(
    publicationPath,
    { evidence, receipt, state },
    signal,
  );
  signal?.throwIfAborted();
  return {
    approvalRequired,
    approvalRequestId,
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
      "Usage: node bootstrap.mjs --control-plane URL --output PATH [--data-plane URL]",
    );
  }
  const result = await bootstrapTetris({
    controlPlaneUrl: values.get("control-plane") ?? "http://127.0.0.1:5081",
    dataPlaneUrl: values.get("data-plane") ?? "http://127.0.0.1:5080",
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
