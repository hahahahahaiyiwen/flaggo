import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  createFlaggoClient,
  type DecisionDefinitionBundle,
} from "@flaggo/sdk";

import { AdaptiveWorker } from "./adaptive-worker.js";
import { LocalTelemetrySink } from "./telemetry.js";

interface ExtractionArtifact {
  bundle: DecisionDefinitionBundle;
}

interface ServiceConnection {
  controlPlaneUrl: string;
  dataPlaneUrl: string;
  telemetryPath: string;
}

function hasArgument(name: string): boolean {
  return process.argv.slice(2).includes(name);
}

function argumentValue(name: string): string | undefined {
  const index = process.argv.slice(2).indexOf(name);
  return index < 0 ? undefined : process.argv.slice(2)[index + 1];
}

export async function runMain(): Promise<void> {
  const exampleRoot = resolve(import.meta.dirname, "..");
  const repositoryRoot = resolve(exampleRoot, "../..");
  const serviceFile = argumentValue("--service-file") ??
    resolve(repositoryRoot, ".flaggo/adaptive-worker/service.json");
  const artifact = JSON.parse(
    await readFile(
      resolve(exampleRoot, "generated/definitions.json"),
      "utf8",
    ),
  ) as ExtractionArtifact;
  const service = JSON.parse(
    await readFile(serviceFile, "utf8"),
  ) as ServiceConnection;
  const allowLocalFallback = hasArgument("--allow-local-fallback");
  const client = await createFlaggoClient({
    appId: artifact.bundle.application.id,
    environment: artifact.bundle.application.environment,
    dataPlaneUrl: service.dataPlaneUrl,
    dataPlaneCredential: { mode: "local-development" },
    controlPlane: {
      mode: "startup-register",
      url: service.controlPlaneUrl,
      bundle: artifact.bundle,
      credential: { mode: "local-development" },
    },
    availabilityFallback: allowLocalFallback
      ? { mode: "local-default", retries: 0 }
      : { mode: "disabled", retries: 0 },
  });
  const telemetry = new LocalTelemetrySink(service.telemetryPath);
  const worker = new AdaptiveWorker(client, telemetry);
  const results = await worker.runProfiles([
    "steady",
    "burst",
    "slow-downstream",
    "recovery",
  ]);

  process.stdout.write(`${JSON.stringify({
    status: "completed",
    receipt: client.definitions.getRegistrationReceipt(),
    profiles: results.map((result) => ({
      profile: result.profile,
      queuePressure: result.queuePressure,
      appliedBatchSize: result.appliedBatchSize,
      processedCount: result.processedItemIds.length,
      queueDepthAfter: result.queueDepthAfter,
      source: result.decision.source,
      decisionMode: result.decision.decisionMode,
      exposureId: result.confirmation?.exposureId,
    })),
    telemetryEvents: telemetry.events.length,
    telemetryPath: service.telemetryPath,
  }, null, 2)}\n`);
}

if (
  process.argv[1] !== undefined &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await runMain();
}
