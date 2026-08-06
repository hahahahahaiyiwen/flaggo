import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  bundleDigest,
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

    // Intentional artifact changes must pass the frozen-schema conformance gate,
    // then update both SDK-computed identity pins in the same reviewed change.
    expect(bundleDigest(bundle)).toBe(
      "sha256:24c061cdab45dcc28420945c6b5008f327886ce85bb3e41bb67c62a801d8a6c8",
    );
    expect(contractDigest(definition)).toBe(
      "sha256:e801be125f7e6b6406feb89092117cdbd2016975a38ee8b03421f5640229d6a6",
    );
    expect(definition.key).toBe("tetris.dropInterval");
    expect(definition).not.toHaveProperty("requestedApproval");
    expect(bundle.definitions[0]).not.toHaveProperty("contractDigest");
    expect(bundle.definitions[0]).not.toHaveProperty("revision");
    expect(bundle.signals?.every((signal) => signal.schemaDigest === undefined))
      .toBe(true);
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
