import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import { parse } from "yaml";
import { compileManifest, parseManifest } from "../src/manifest.js";
import { runManifestCli } from "../src/manifest-cli.js";

it("compiles native OTel bindings without request-owned inputs or producer declarations", async () => {
  const root = resolve(import.meta.dirname, "../../../examples/otel-evidence");
  const source = resolve(root, "decision-manifest.json");
  await runManifestCli([
    "check", source,
    "--bundle", resolve(root, "generated/definitions.json"),
    "--catalog", resolve(root, "generated/catalog.ts"),
  ]);
  const compiled = compileManifest(parseManifest(readFileSync(source, "utf8")));
  const definition = compiled.bundle.decisions["worker.batchSize"]!;
  expect(Object.values(definition.inputs!).every((input) => input.source === "evidence")).toBe(true);
  expect(Object.keys(definition.evidence!)).toEqual([
    "queuePressure", "processingTime", "retryCount", "failureCount", "appliedOutcome",
  ]);
  expect(definition.evidence?.appliedOutcome?.attribution.kind).toBe("confirmed-exposure");
  expect(compiled.bundle).not.toHaveProperty("signals");
  const configuration = parse(readFileSync(resolve(root, "collector.yaml"), "utf8"));
  for (const signal of ["metrics", "traces", "logs"]) {
    expect(configuration.service.pipelines[`${signal}/existing`].exporters).toEqual(["debug/existing"]);
    expect(configuration.service.pipelines[`${signal}/flaggo`].exporters).toEqual(["otlp_http/flaggo"]);
  }
  expect(configuration.exporters["otlp_http/flaggo"]).toMatchObject({
    encoding: "proto", compression: "gzip",
  });
});
