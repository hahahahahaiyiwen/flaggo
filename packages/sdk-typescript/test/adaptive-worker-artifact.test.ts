import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  bundleDigest,
  normalizeBundle,
  type DecisionDefinitionBundle,
  type NumberDecisionDefinition,
} from "../src/index.js";
import { runExtractorCli } from "../src/extract-cli.js";

interface ExtractionArtifact {
  format: "flaggo.static-extraction/v1";
  bundle: DecisionDefinitionBundle;
  bundleDigest: string;
  descriptors: Array<{
    decisionKey: string;
    contractDigest: string;
    sourceFile: string;
    line: number;
    column: number;
  }>;
}

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const exampleRoot = resolve(repositoryRoot, "examples/adaptive-worker");
const committedArtifactPath = resolve(
  exampleRoot,
  "generated/definitions.json",
);

describe("Phase 2.5 adaptive-worker artifact", () => {
  it(
    "is deterministic and declares the canonical queue decision contract",
    async () => {
      const temporaryDirectory = mkdtempSync(
        resolve(tmpdir(), "flaggo-adaptive-worker-"),
      );
      const regeneratedPath = resolve(temporaryDirectory, "definitions.json");

      try {
        await runExtractorCli([
          "--project",
          resolve(exampleRoot, "tsconfig.json"),
          "--app",
          "adaptive-worker-demo",
          "--environment",
          "dev",
          "--out",
          regeneratedPath,
          "--repository",
          "github.com/hahahahahaiyiwen/flaggo",
          "--source-path",
          "examples/adaptive-worker/src",
        ]);

        const committed = JSON.parse(
          readFileSync(committedArtifactPath, "utf8"),
        ) as ExtractionArtifact;
        const regenerated = JSON.parse(
          readFileSync(regeneratedPath, "utf8"),
        ) as ExtractionArtifact;
        expect(regenerated).toEqual(committed);

        const normalized = normalizeBundle(committed.bundle);
        const definition =
          normalized.definitions[0] as NumberDecisionDefinition;
        expect(committed.format).toBe("flaggo.static-extraction/v1");
        expect(committed.bundleDigest).toBe(bundleDigest(committed.bundle));
        expect(committed.bundle.application).toEqual({
          id: "adaptive-worker-demo",
          environment: "dev",
        });
        expect(committed.descriptors).toEqual([
          expect.objectContaining({
            decisionKey: "demo.workerBatchSize",
            sourceFile: "src/adaptive-worker.ts",
          }),
        ]);
        expect(definition.actionSpace).toEqual({
          type: "number",
          min: 1,
          max: 10,
          step: 1,
          default: 3,
        });
        expect(definition.fallback).toEqual({
          value: 3,
          reason: "safe_default_worker_batch_size",
        });
        expect(definition.inference).toEqual({
          target: "cohort",
          inputs: [{ key: "demo.queuePressure" }],
          fallbackOrder: ["global"],
        });
        expect(definition.policy).toEqual({
          kind: "inline",
          constraints: [
            { kind: "cooldown", seconds: 1 },
            { kind: "max-delta", value: 3 },
            { kind: "min-evidence-quality", value: 0.8 },
            { kind: "number-bounds", min: 1, max: 10 },
          ],
          clientFallback: {
            requiredEvidenceUnavailable: "allow",
          },
        });
        expect(
          normalized.signals?.map(({ key }) => key).sort(),
        ).toEqual([
          "demo.itemCompleted",
          "demo.itemEnqueued",
          "demo.processingLatencyMs",
          "demo.queueDepth",
          "demo.queuePressure",
        ]);
      } finally {
        rmSync(temporaryDirectory, { recursive: true, force: true });
      }
    },
    15_000,
  );
});
