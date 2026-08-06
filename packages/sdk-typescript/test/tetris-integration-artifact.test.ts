import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  contractDigest,
  normalizeBundle,
  type DecisionDefinitionBundle,
  type NumberDecisionDefinition,
} from "../src/index.js";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const bundle = JSON.parse(
  readFileSync(
    resolve(
      repositoryRoot,
      "examples/tetris-integration/tetris-definition-bundle.json",
    ),
    "utf8",
  ),
) as DecisionDefinitionBundle;

describe("Phase 3 Tetris artifact", () => {
  it("is the canonical four-input adaptive definition and outcome contract", () => {
    const normalized = normalizeBundle(bundle);
    const definition = normalized.definitions[0] as NumberDecisionDefinition;

    expect(definition.key).toBe("tetris.dropInterval");
    expect(definition).not.toHaveProperty("requestedApproval");
    expect(definition.actionSpace).toEqual({
      type: "number",
      min: 200,
      max: 1500,
      step: 50,
      default: 800,
    });
    expect(definition.fallback.value).toBe(800);
    expect(definition.inference?.inputs?.map(({ key }) => key)).toEqual([
      "tetris.boardPressure",
      "tetris.currentLevel",
      "tetris.recentPlacementTimeMs",
      "tetris.recoveryFailures",
    ]);
    expect(definition.policy).toEqual({
      kind: "inline",
      constraints: [
        { kind: "cooldown", seconds: 20 },
        { kind: "max-delta", value: 50 },
        { kind: "min-evidence-quality", value: 0.7 },
        { kind: "number-bounds", min: 200, max: 1500 },
      ],
    });
    expect(contractDigest(definition)).not.toBe(
      "sha256:6eadd7bd76b36ae06e89376d57107da83fdcabf07ff58c528ae97fddb7f08ee9",
    );
    expect(
      normalized.signals?.find(({ key }) => key === "tetris.outcomeObserved"),
    ).toMatchObject({
      kind: "event",
      fields: {
        decisionId: "string",
        exposureId: "string",
        outcome: "string",
        dropIntervalMs: "number",
      },
    });
  });
});
