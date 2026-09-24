import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import { compileManifest, parseManifest } from "../src/manifest.js";
import { runManifestCli } from "../src/manifest-cli.js";

it("keeps the worker publication bundle and typed catalog generated from one manifest", async () => {
  const root = resolve(import.meta.dirname, "../../../examples/adaptive-worker");
  const source = resolve(root, "decision-manifest.json");
  const bundle = resolve(root, "generated/definitions.json");
  const catalog = resolve(root, "src/generated/catalog.ts");
  await runManifestCli(["check", source, "--bundle", bundle, "--catalog", catalog]);
  const compiled = compileManifest(parseManifest(readFileSync(source, "utf8")));
  const definition = compiled.bundle.decisions["demo.workerBatchSize"]!;
  expect(definition.result).toEqual({ type: "number", min: 1, max: 10, step: 1, default: 3 });
  expect(definition.inputs).toEqual({
    queuePressure: { source: "request", type: "number", meaning: expect.any(String), unit: "1", range: [0, 1] },
  });
  expect(definition.policy).toEqual({
    kind: "inline",
    constraints: [
      { kind: "cooldown", seconds: 1 },
      { kind: "max-delta", value: 3 },
      { kind: "number-bounds", min: 1, max: 10 },
    ],
  });
  expect(compiled.bundle).not.toHaveProperty("signals");
  expect(definition).not.toHaveProperty("onlineStrategy");
});
