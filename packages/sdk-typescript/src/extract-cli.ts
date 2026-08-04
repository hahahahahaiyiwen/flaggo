#!/usr/bin/env node

import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import * as ts from "typescript";

import { extractDecisionBundle } from "./extractor.js";

interface Arguments {
  project: string;
  app: string;
  environment: string;
  out: string;
  repository?: string;
  sourcePath?: string;
}

function usage(): never {
  throw new Error(
    "Usage: flaggo-extract --project <tsconfig.json> --app <id> "
      + "--environment <name> --out <file> "
      + "[--repository <name>] [--source-path <path>]",
  );
}

function parseArguments(values: string[]): Arguments {
  const parsed = new Map<string, string>();
  const allowed = new Set([
    "project",
    "app",
    "environment",
    "out",
    "repository",
    "source-path",
  ]);
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (key === undefined || !key.startsWith("--") || value === undefined) {
      usage();
    }
    const name = key.slice(2);
    if (!allowed.has(name) || parsed.has(name)) usage();
    parsed.set(name, value);
  }
  const project = parsed.get("project");
  const app = parsed.get("app");
  const environment = parsed.get("environment");
  const out = parsed.get("out");
  if (
    project === undefined
    || app === undefined
    || environment === undefined
    || out === undefined
    || app.length === 0
    || environment.length === 0
  ) {
    usage();
  }
  return {
    project,
    app,
    environment,
    out,
    ...(parsed.get("repository") === undefined
      ? {}
      : { repository: parsed.get("repository")! }),
    ...(parsed.get("source-path") === undefined
      ? {}
      : { sourcePath: parsed.get("source-path")! }),
  };
}

export async function runExtractorCli(values: string[]): Promise<void> {
  const args = parseArguments(values);
  const project = resolve(args.project);
  const configFile = ts.readConfigFile(project, ts.sys.readFile);
  if (configFile.error !== undefined) {
    throw new Error(
      ts.flattenDiagnosticMessageText(configFile.error.messageText, "\n"),
    );
  }
  const rootDir = dirname(project);
  const config = ts.parseJsonConfigFileContent(
    configFile.config,
    ts.sys,
    rootDir,
  );
  if (config.errors.length > 0) {
    throw new Error(
      config.errors
        .map((error) =>
          ts.flattenDiagnosticMessageText(error.messageText, "\n")
        )
        .join("\n"),
    );
  }
  const result = extractDecisionBundle({
    fileNames: config.fileNames,
    rootDir,
    application: { id: args.app, environment: args.environment },
    source: {
      ...(args.repository === undefined
        ? {}
        : { repository: args.repository }),
      ...(args.sourcePath === undefined ? {} : { path: args.sourcePath }),
    },
    compilerOptions: config.options,
  });
  const output = resolve(args.out);
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(result, null, 2)}\n`, "utf8");
}

const entryPoint = process.argv[1];
if (
  entryPoint !== undefined
  && import.meta.url === pathToFileURL(resolve(entryPoint)).href
) {
  await runExtractorCli(process.argv.slice(2));
}
