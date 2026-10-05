import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  createDecisionClient,
  type DecisionBindings,
} from "@flaggo/sdk/runtime";
import {
  parseFlaggoRuntimeConfiguration,
} from "@flaggo/sdk/configuration";

import { AdaptiveWorker, type WorkerDecisions } from "./adaptive-worker.js";
import { AdaptiveWorkerTelemetry } from "./telemetry.js";

function argumentValue(name: string): string | undefined {
  const index = process.argv.slice(2).indexOf(name);
  return index < 0 ? undefined : process.argv.slice(2)[index + 1];
}

export async function runMain(): Promise<void> {
  const exampleRoot = resolve(import.meta.dirname, "..");
  const repositoryRoot = resolve(exampleRoot, "../..");
  const runtimeConfigFile = argumentValue("--runtime-config") ??
    resolve(repositoryRoot, ".flaggo/adaptive-worker/flaggo.runtime.json");
  const runtimeConfig = parseFlaggoRuntimeConfiguration<
    DecisionBindings<WorkerDecisions>
  >(JSON.parse(await readFile(runtimeConfigFile, "utf8")));
  const telemetry = new AdaptiveWorkerTelemetry({
    runtimeConfig,
  });
  const client = createDecisionClient<WorkerDecisions>({
    runtimeConfig,
    telemetry: { logger: telemetry.flaggoLogger },
  });
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
      profiles: results.map((result) => ({
        profile: result.profile,
        queuePressure: result.queuePressure,
        appliedBatchSize: result.appliedBatchSize,
        processedCount: result.processedItemIds.length,
        queueDepthAfter: result.queueDepthAfter,
        evaluation: result.decision.evaluation,
        executableDigest: result.decision.executableDigest,
      })),
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
