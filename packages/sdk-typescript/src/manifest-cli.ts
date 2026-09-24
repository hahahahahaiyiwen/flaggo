#!/usr/bin/env node

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { compileManifest, parseManifest } from "./manifest.js";

export async function runManifestCli(args: string[]): Promise<void> {
  const [command, manifestPath, ...options] = args;
  const values = new Map<string, string>();
  for (let index = 0; index < options.length; index += 2) {
    const key = options[index];
    const value = options[index + 1];
    if (key === undefined || value === undefined
      || !["--bundle", "--catalog"].includes(key) || values.has(key)) {
      throw new Error("Usage: flaggo-manifest <build|check> <manifest.json> --bundle <bundle.json> --catalog <catalog.ts>");
    }
    values.set(key, value);
  }
  const bundlePath = values.get("--bundle");
  const catalogPath = values.get("--catalog");
  if (!["build", "check"].includes(command ?? "") || manifestPath === undefined
    || bundlePath === undefined || catalogPath === undefined) {
    throw new Error("Usage: flaggo-manifest <build|check> <manifest.json> --bundle <bundle.json> --catalog <catalog.ts>");
  }
  const paths = [manifestPath, bundlePath, catalogPath].map((path) => resolve(path));
  if (new Set(paths.map((path) => process.platform === "win32" ? path.toLowerCase() : path)).size !== 3) {
    throw new Error("Manifest, publication bundle, and catalog must be different files.");
  }
  const compiled = compileManifest(parseManifest(await readFile(paths[0]!, "utf8")));
  const artifacts: readonly [string, string][] = [
    [paths[1]!, `${JSON.stringify(compiled.bundle, null, 2)}\n`],
    [paths[2]!, compiled.catalogSource],
  ];
  for (const [path, expected] of artifacts) {
    if (command === "check") {
      if ((await readFile(path, "utf8")).replaceAll("\r\n", "\n") !== expected) {
        throw new Error(`Generated artifact is stale: ${path}`);
      }
    } else {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, expected, "utf8");
    }
  }
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await runManifestCli(process.argv.slice(2));
}
