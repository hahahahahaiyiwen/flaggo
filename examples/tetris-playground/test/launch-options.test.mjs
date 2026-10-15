import assert from "node:assert/strict";
import test from "node:test";
import { parsePlaygroundArguments, validatePlaygroundCredentials } from "../launch-options.mjs";
import { analysisInferenceEnvironment } from "../../tetris-integration/analysis-inference.mjs";

test("live defaults to Copilot and benchmark defaults to deterministic", () => {
  assert.deepEqual(parsePlaygroundArguments([]),
    { mode: "live", analysisMode: "copilot", retain: false });
  assert.deepEqual(parsePlaygroundArguments(["--mode=benchmark"]),
    { mode: "benchmark", analysisMode: "deterministic", retain: false });
  assert.deepEqual(parsePlaygroundArguments(["--mode=benchmark", "--analysis=copilot", "--retain"]),
    { mode: "benchmark", analysisMode: "copilot", retain: true });
});

const byok = {
  FLAGGO_ANALYSIS_INFERENCE: "byok",
  FLAGGO_ANALYSIS_MODEL: "selected-deployment",
  OPENAI_BASE_URL: "https://example.openai.azure.com/openai/v1/",
  FLAGGO_ANALYSIS_BYOK_WIRE_API: "responses",
  OPENAI_API_KEY: "test-provider-secret",
};

test("BYOK playground starts without GitHub credentials and ignores ambient tokens", () => {
  validatePlaygroundCredentials(parsePlaygroundArguments([]), byok);
  assert.deepEqual(analysisInferenceEnvironment({
    ...byok, GH_TOKEN: "", GITHUB_TOKEN: "",
  }), byok);
});

test("BYOK never falls back on missing, malformed or unsupported configuration", () => {
  for (const name of Object.keys(byok)) {
    const environment = { ...byok };
    delete environment[name];
    assert.throws(() => analysisInferenceEnvironment(environment));
  }
  for (const [name, value] of [
    ["FLAGGO_ANALYSIS_INFERENCE", "automatic"],
    ["FLAGGO_ANALYSIS_BYOK_WIRE_API", "unknown"],
    ["OPENAI_BASE_URL", "https://user:test-provider-secret@example.com/v1"],
    ["OPENAI_BASE_URL", "https://example.com/v1?key=test-provider-secret"],
    ["OPENAI_BASE_URL", "https://example.com/v1?"],
    ["OPENAI_BASE_URL", "https://example.com/v1#"],
    ["OPENAI_BASE_URL", "http://remote.example/v1"],
    ["OPENAI_BASE_URL", "not-a-url"],
    ["OPENAI_API_KEY", ""],
  ]) {
    assert.throws(() => analysisInferenceEnvironment({ ...byok, [name]: value }), (error) => {
      assert.ok(!error.message.includes("test-provider-secret"));
      return true;
    });
  }
});

test("explicit local endpoints can omit keys and GitHub mode preserves its token precedence", () => {
  const environment = {
    ...byok,
    OPENAI_BASE_URL: "http://127.0.0.1:11434/v1",
    FLAGGO_ANALYSIS_BYOK_WIRE_API: "completions",
  };
  delete environment.OPENAI_API_KEY;
  assert.deepEqual(analysisInferenceEnvironment(environment), environment);
  assert.deepEqual(analysisInferenceEnvironment({ GH_TOKEN: "test-token" }), {
    FLAGGO_ANALYSIS_INFERENCE: "github", GH_TOKEN: "test-token",
  });
  assert.deepEqual(analysisInferenceEnvironment({ GITHUB_TOKEN: "test-token" }), {
    FLAGGO_ANALYSIS_INFERENCE: "github", GH_TOKEN: "test-token",
  });
  assert.deepEqual(analysisInferenceEnvironment({
    GH_TOKEN: "preferred-token", GITHUB_TOKEN: "other-token",
    OPENAI_BASE_URL: "not-a-url", OPENAI_API_KEY: "",
  }), { FLAGGO_ANALYSIS_INFERENCE: "github", GH_TOKEN: "preferred-token" });
  assert.throws(() => analysisInferenceEnvironment({
    GH_TOKEN: "", GITHUB_TOKEN: "other-token",
  }), /requires GH_TOKEN or GITHUB_TOKEN/u);
});

test("invalid combinations fail rather than silently substituting a provider", () => {
  assert.throws(() => parsePlaygroundArguments(["--analysis=deterministic"]), /requires Copilot/u);
  for (const argument of ["--mode=unknown", "--analysis=unknown", "--unknown"]) {
    assert.throws(() => parsePlaygroundArguments([argument]));
  }
  assert.throws(() => validatePlaygroundCredentials(parsePlaygroundArguments([]), {}),
    /requires GH_TOKEN or GITHUB_TOKEN/u);
  validatePlaygroundCredentials(parsePlaygroundArguments(["--mode=benchmark"]), {});
  validatePlaygroundCredentials(parsePlaygroundArguments([]), { GH_TOKEN: "test-token" });
  assert.throws(() => analysisInferenceEnvironment({
    GH_TOKEN: "test-token", FLAGGO_ANALYSIS_BYOK_WIRE_API: "responses",
  }), /requires FLAGGO_ANALYSIS_INFERENCE=byok/u);
});
