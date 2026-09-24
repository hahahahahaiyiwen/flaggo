import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  createFlaggoClient,
  type RegistrationReceipt,
} from "@flaggo/sdk";

import { AdaptiveWorker } from "./adaptive-worker.js";
import { LocalOtelLogs } from "./telemetry.js";
import { catalog } from "./generated/catalog.js";

interface ServiceConnection {
  controlPlaneUrl: string;
  dataPlaneUrl: string;
  telemetryPath: string;
  receipt: RegistrationReceipt;
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
  const service = JSON.parse(
    await readFile(serviceFile, "utf8"),
  ) as ServiceConnection;
  const allowLocalFallback = hasArgument("--allow-local-fallback");
  const client = createFlaggoClient({
    catalog,
    receipt: service.receipt,
    dataPlaneUrl: service.dataPlaneUrl,
    dataPlaneCredential: { mode: "local-development" },
    availabilityFallback: allowLocalFallback
      ? { mode: "local-default", retries: 0 }
      : { mode: "disabled", retries: 0 },
  });
  const telemetry = new LocalOtelLogs(service.telemetryPath);
  try {
    const worker = new AdaptiveWorker(client, telemetry);
    const results = await worker.runProfiles([
      "steady",
      "burst",
      "slow-downstream",
      "recovery",
    ]);

    await telemetry.flush();
    process.stdout.write(`${JSON.stringify({
      status: "completed",
      receipt: service.receipt,
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
  } finally {
    await telemetry.shutdown();
  }
}

if (
  process.argv[1] !== undefined &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await runMain();
}
