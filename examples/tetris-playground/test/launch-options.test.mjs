import assert from "node:assert/strict";
import test from "node:test";
import { parsePlaygroundArguments, validatePlaygroundCredentials } from "../launch-options.mjs";

test("live defaults to Copilot and benchmark defaults to deterministic", () => {
  assert.deepEqual(parsePlaygroundArguments([]),
    { mode: "live", analysisMode: "copilot", retain: false });
  assert.deepEqual(parsePlaygroundArguments(["--mode=benchmark"]),
    { mode: "benchmark", analysisMode: "deterministic", retain: false });
  assert.deepEqual(parsePlaygroundArguments(["--mode=benchmark", "--analysis=copilot", "--retain"]),
    { mode: "benchmark", analysisMode: "copilot", retain: true });
});

test("invalid combinations fail rather than silently substituting a provider", () => {
  assert.throws(() => parsePlaygroundArguments(["--analysis=deterministic"]), /requires Copilot/u);
  for (const argument of ["--mode=unknown", "--analysis=unknown", "--unknown"]) {
    assert.throws(() => parsePlaygroundArguments([argument]));
  }
  assert.throws(() => validatePlaygroundCredentials(parsePlaygroundArguments([]), {}),
    /requires FLAGGO_ANALYSIS_GITHUB_TOKEN/u);
  validatePlaygroundCredentials(parsePlaygroundArguments(["--mode=benchmark"]), {});
  validatePlaygroundCredentials(parsePlaygroundArguments([]), { GH_TOKEN: "test-token" });
});
