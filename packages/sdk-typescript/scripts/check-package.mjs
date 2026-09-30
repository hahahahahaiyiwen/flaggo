import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

import { build } from "esbuild";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const metadata = JSON.parse(
  await readFile(resolve(packageRoot, "package.json"), "utf8"),
);
const entries = [
  {
    name: "runtime",
    path: resolve(packageRoot, metadata.exports["./runtime"].import),
    factory: "createDecisionClient",
  },
  {
    name: "management",
    path: resolve(packageRoot, metadata.exports["./management"].import),
    factory: "createContractClient",
  },
  {
    name: "opentelemetry",
    path: resolve(packageRoot, metadata.exports["./opentelemetry"].import),
    factory: "createFlaggoSpanProcessor",
  },
];

assert.deepEqual(
  Object.keys(metadata.dependencies ?? {}).sort(),
  [
    "@opentelemetry/core",
    "@opentelemetry/exporter-logs-otlp-http",
    "@opentelemetry/exporter-metrics-otlp-http",
    "@opentelemetry/exporter-trace-otlp-http",
    "@opentelemetry/otlp-exporter-base",
  ],
);
assert.deepEqual(
  Object.keys(metadata.peerDependencies ?? {}).sort(),
  [
    "@opentelemetry/api",
    "@opentelemetry/api-logs",
    "@opentelemetry/sdk-logs",
    "@opentelemetry/sdk-metrics",
    "@opentelemetry/sdk-trace",
  ],
);
assert.equal(metadata.sideEffects, false);
assert.equal(metadata.license, "MIT");
assert.equal(metadata.exports["."], undefined);
await access(resolve(packageRoot, "LICENSE"));

for (const entry of entries) {
  await access(entry.path);
  const exports = await import(pathToFileURL(entry.path));
  assert.equal(exports.SDK_VERSION, metadata.version);
  assert.equal(typeof exports[entry.factory], "function");

  const platforms = entry.name === "opentelemetry"
    ? ["browser", "node"]
    : ["browser", "neutral"];
  for (const platform of platforms) {
    const result = await build({
      bundle: true,
      entryPoints: [entry.path],
      format: "esm",
      mainFields: platform === "browser"
        ? ["browser", "module", "main"]
        : ["module", "main"],
      platform,
      target: "es2022",
      write: false,
    });
    assert.equal(result.outputFiles.length, 1);
    const output = new TextDecoder().decode(result.outputFiles[0].contents);
    if (platform !== "node") {
      assert.doesNotMatch(output, /\bfrom\s+["']node:/u);
    }
  }
}
