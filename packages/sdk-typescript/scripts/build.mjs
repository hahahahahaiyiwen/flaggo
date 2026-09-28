import { execFile } from "node:child_process";
import { mkdir, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const execFileAsync = promisify(execFile);
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "..", "..");
const outputRoot = resolve(packageRoot, "dist");
const tsc = resolve(repositoryRoot, "node_modules", "typescript", "bin", "tsc");

await rm(outputRoot, { force: true, recursive: true });
await mkdir(outputRoot, { recursive: true });

await execFileAsync(process.execPath, [
  tsc,
  "-p",
  resolve(packageRoot, "tsconfig.build.json"),
]);

await build({
  absWorkingDir: packageRoot,
  bundle: true,
  entryNames: "[dir]/[name]",
  entryPoints: [
    "src/runtime/index.ts",
    "src/management/index.ts",
  ],
  external: [
    "@opentelemetry/api-logs",
    "@opentelemetry/exporter-logs-otlp-proto",
    "@opentelemetry/resources",
    "@opentelemetry/sdk-logs",
  ],
  format: "esm",
  legalComments: "none",
  outbase: "src",
  outdir: "dist",
  platform: "neutral",
  sourcemap: true,
  splitting: false,
  target: "es2022",
});
