export function parsePlaygroundArguments(arguments_) {
  let mode = "live";
  let analysisMode;
  let retain = false;
  for (const argument of arguments_) {
    if (argument === "--retain") retain = true;
    else if (argument.startsWith("--mode=")) {
      mode = argument.slice("--mode=".length);
      if (mode !== "live" && mode !== "benchmark") {
        throw new Error("--mode must be 'live' or 'benchmark'.");
      }
    } else if (argument.startsWith("--analysis=")) {
      analysisMode = argument.slice("--analysis=".length);
      if (analysisMode !== "deterministic" && analysisMode !== "copilot") {
        throw new Error("--analysis must be 'deterministic' or 'copilot'.");
      }
    } else throw new Error(`Unknown playground argument '${argument}'.`);
  }
  analysisMode ??= mode === "live" ? "copilot" : "deterministic";
  if (mode === "live" && analysisMode !== "copilot") {
    throw new Error("Live mode requires Copilot analysis; use --mode=benchmark for deterministic analysis.");
  }
  return { mode, analysisMode, retain };
}

export function validatePlaygroundCredentials(options, environment = process.env) {
  if (options.analysisMode !== "copilot") return;
  analysisInferenceEnvironment(environment);
}
import { analysisInferenceEnvironment } from "../tetris-integration/analysis-inference.mjs";
