import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  RequiresApprovalError,
  createFlaggoClient,
} from "../../packages/sdk-typescript/dist/index.js";
import { publishJsonGeneration } from "../tetris-integration/bootstrap.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
export const extractionArtifactPath = resolve(
  exampleDirectory,
  "generated/definitions.json",
);
export const decisionKey = "demo.workerBatchSize";
export const strategyId = "strategy-adaptive-worker-pressure-v1";

export async function loadExtractionArtifact(signal) {
  return JSON.parse(
    await readFile(extractionArtifactPath, { encoding: "utf8", signal }),
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
          inputSignalKey: "demo.queuePressure",
          threshold: 0.7,
          valueAtOrAbove: 6,
          valueBelow: 3,
        },
        lastChangedAt: lastChangedAt.toISOString(),
      },
    ],
  };
}

export function createAdaptiveWorkerEvidence() {
  return {
    version: 1,
    evidenceByStrategy: {
      [strategyId]: {
        evidenceQuality: 0.95,
        modelUncertainty: 0.05,
        expectedOutcome: 0.9,
        sampleSize: 100,
        details: {
          source: "phase2.5-local-fixture",
          workload: "deterministic-in-memory-queue",
        },
      },
    },
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
  const artifact = await loadExtractionArtifact(signal);
  const fetchWithAbort = (input, init = {}) =>
    fetchImpl(
      input,
      signal === undefined ? init : { ...init, signal },
    );
  const config = {
    appId: artifact.bundle.application.id,
    environment: artifact.bundle.application.environment,
    dataPlaneUrl: "http://127.0.0.1:0",
    controlPlane: {
      mode: "startup-register",
      url: controlPlaneUrl,
      bundle: artifact.bundle,
      credential: { mode: "local-development" },
    },
    fetch: fetchWithAbort,
  };

  let approvalRequired = false;
  let approvalRequestId;
  let client;
  try {
    client = await createFlaggoClient(config);
  } catch (error) {
    if (!(error instanceof RequiresApprovalError)) throw error;
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
          comment: "Trusted Phase 2.5 adaptive-worker local bootstrap.",
        }),
      },
    );
    const approval = await response.json();
    if (!response.ok || approval.status !== "approved") {
      throw new Error(
        `Adaptive-worker definition approval failed with HTTP ${response.status}: ${JSON.stringify(approval)}`,
      );
    }
    client = await createFlaggoClient(config);
  }

  const receipt = client.definitions.getRegistrationReceipt();
  const state = createAdaptiveWorkerState(receipt, { now });
  const evidence = createAdaptiveWorkerEvidence();
  const publication = await publishGeneration(
    publicationPath,
    { evidence, receipt, state },
    signal,
  );
  return {
    approvalRequired,
    approvalRequestId,
    artifact,
    receipt,
    state,
    evidence,
    publication,
  };
}
