import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function inspectIntegration(auditPath, telemetryPath) {
  const audit = await readJsonLines(auditPath);
  const telemetry = await readJsonLines(telemetryPath);
  const decisions = audit
    .filter(({ kind }) => kind === "decision")
    .map(({ record }) => record);
  const exposures = audit
    .filter(({ kind }) => kind === "exposure")
    .map(({ record }) => record);
  const linkedOutcomes = telemetry.filter((event) =>
    event.signal?.key === "tetris.outcomeObserved"
    && exposures.some((exposure) =>
      exposure.exposureId === event.value?.exposureId
      && exposure.decisionId === event.value?.decisionId
    )
  );
  return {
    decisionCount: decisions.length,
    exposureCount: exposures.length,
    linkedOutcomeCount: linkedOutcomes.length,
    strategies: [...new Set(
      decisions.map(({ strategyId }) => strategyId).filter(Boolean),
    )],
    decisionInputs: decisions.map(({ decisionId, inputs }) => ({
      decisionId,
      signalKeys: inputs.map(({ signal }) => signal.key),
    })),
    policyResults: decisions.map(({ decisionId, policy, fallback }) => ({
      decisionId,
      result: policy.result,
      reasons: policy.reasons,
      fallbackSource: fallback.source,
      decisionFallbackUsed: fallback.decisionFallbackUsed,
    })),
    exposures,
    linkedOutcomes,
  };
}

async function readJsonLines(path) {
  try {
    const content = await readFile(path, "utf8");
    return content
      .split(/\r?\n/u)
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line));
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
}

function option(args, name) {
  const index = args.indexOf(name);
  return index < 0 ? undefined : args[index + 1];
}

if (
  process.argv[1] !== undefined
  && pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  const auditPath = option(process.argv, "--audit");
  const telemetryPath = option(process.argv, "--telemetry");
  if (auditPath === undefined || telemetryPath === undefined) {
    throw new Error(
      "Usage: node inspect.mjs --audit PATH --telemetry PATH",
    );
  }
  process.stdout.write(
    `${JSON.stringify(
      await inspectIntegration(auditPath, telemetryPath),
      null,
      2,
    )}\n`,
  );
}
