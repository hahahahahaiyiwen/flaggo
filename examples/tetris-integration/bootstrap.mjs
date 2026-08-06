import { readFile, rename, rm, mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  RequiresApprovalError,
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
export const canonicalBundlePath = resolve(
  exampleDirectory,
  "tetris-definition-bundle.json",
);
export const strategyActivationPath = resolve(
  exampleDirectory,
  "strategy-activation.json",
);

export async function loadCanonicalBundle() {
  return JSON.parse(await readFile(canonicalBundlePath, "utf8"));
}

export async function createActivatedState(receipt, now = new Date()) {
  const activation = JSON.parse(
    await readFile(strategyActivationPath, "utf8"),
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
  statePath,
  receiptPath,
  fetchImpl = globalThis.fetch,
  now = new Date(),
}) {
  const bundle = await loadCanonicalBundle();
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
    fetch: fetchImpl,
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
    const response = await fetchImpl(
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
  const state = await createActivatedState(receipt, now);
  await Promise.all([
    writeJsonAtomic(receiptPath, receipt),
    writeJsonAtomic(statePath, state),
  ]);
  return {
    approvalRequired,
    approvalRequestId,
    receipt,
    state,
  };
}

export async function writeJsonAtomic(filePath, value) {
  const absolutePath = resolve(filePath);
  const directory = dirname(absolutePath);
  const stagingPath = `${absolutePath}.${process.pid}.tmp`;
  await mkdir(directory, { recursive: true });
  try {
    await writeFile(stagingPath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
    await rename(stagingPath, absolutePath);
  } finally {
    await rm(stagingPath, { force: true });
  }
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
  const statePath = values.get("state");
  const receiptPath = values.get("receipt");
  if (statePath === undefined || receiptPath === undefined) {
    throw new Error(
      "Usage: node bootstrap.mjs --control-plane URL --state PATH --receipt PATH [--data-plane URL]",
    );
  }
  const result = await bootstrapTetris({
    controlPlaneUrl: values.get("control-plane") ?? "http://127.0.0.1:5081",
    dataPlaneUrl: values.get("data-plane") ?? "http://127.0.0.1:5080",
    statePath,
    receiptPath,
  });
  process.stdout.write(`${JSON.stringify({
    status: "ready",
    approvalRequired: result.approvalRequired,
    approvalRequestId: result.approvalRequestId,
    bundleDigest: result.receipt.bundleDigest,
    revision:
      result.receipt.acceptedDefinitions["tetris.dropInterval"].revision,
    statePath: resolve(statePath),
    receiptPath: resolve(receiptPath),
  }, null, 2)}\n`);
}

if (
  process.argv[1] !== undefined
  && pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
