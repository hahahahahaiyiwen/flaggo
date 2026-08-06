import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function inspectIntegration(auditPath, telemetryPath) {
  const audit = await readAuditJsonLines(auditPath);
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

async function readAuditJsonLines(path) {
  const segmentsPath = `${path}.d`;
  let manifestContent;
  try {
    manifestContent = await readFile(
      join(segmentsPath, "manifest.json"),
      "utf8",
    );
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
    return readJsonLines(path);
  }
  const directory = join(segmentsPath, "segments");
  const manifest = JSON.parse(manifestContent);
  if (
    manifest?.version !== 1
    || !Array.isArray(manifest.segments)
    || manifest.segments.length === 0
  ) {
    throw new Error("Audit segment manifest is invalid.");
  }
  const listed = [...manifest.segments]
    .sort((left, right) => left.sequence - right.sequence);
  const segments = await Promise.all(
    listed.map((segment) =>
      readManifestSegment(join(directory, segment.fileName), segment)
    ),
  );
  return segments.flat().filter(({ kind }) => kind !== "segment");
}

async function readManifestSegment(path, catalog) {
  const bytes = await readFile(path);
  const hash =
    `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  if (
    bytes.byteLength !== catalog.length
    || hash !== catalog.contentHash
  ) {
    throw new Error("Audit segment does not match its durable manifest.");
  }
  return parseJsonLines(bytes.toString("utf8"));
}

async function readJsonLines(path) {
  try {
    const content = await readFile(path, "utf8");
    return parseJsonLines(content);
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
}

function parseJsonLines(content) {
  return content
    .split(/\r?\n/u)
    .filter((line) => line.trim().length > 0)
    .map((line) => JSON.parse(line));
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
