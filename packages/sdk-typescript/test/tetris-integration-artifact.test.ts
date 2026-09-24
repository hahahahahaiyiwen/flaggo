import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import { compileManifest, parseManifest } from "../src/manifest.js";
import { runManifestCli } from "../src/manifest-cli.js";

it("keeps the current Tetris value envelope and four required live inputs in one manifest", async () => {
  const root = resolve(import.meta.dirname, "../../../examples/tetris-integration");
  const source = resolve(root, "tetris-definition-bundle.json");
  await runManifestCli(["check", source,
    "--bundle", resolve(root, "generated/definitions.json"),
    "--catalog", resolve(root, "generated/catalog.ts")]);
  const { bundle } = compileManifest(parseManifest(readFileSync(source, "utf8")));
  const definition = bundle.decisions["tetris.dropInterval"]!;
  expect(definition.result).toEqual({ type: "number", min: 200, max: 1500, step: 50, default: 800 });
  expect(Object.keys(definition.inputs ?? {}).sort()).toEqual([
    "boardPressure", "currentLevel", "recentPlacementTimeMs", "recoveryFailures",
  ]);
  expect(Object.values(definition.inputs ?? {}).every((input) => input.source === "request")).toBe(true);
  expect(definition.policy).toEqual({
    kind: "inline",
    constraints: [
      { kind: "cooldown", seconds: 20 },
      { kind: "max-delta", value: 50 },
      { kind: "number-bounds", min: 200, max: 1500 },
    ],
  });
  expect(bundle).not.toHaveProperty("signals");
  expect(definition).not.toHaveProperty("definitionId");
  expect(definition).not.toHaveProperty("fallback");
});
