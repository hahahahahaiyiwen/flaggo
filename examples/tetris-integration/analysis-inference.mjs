const byokVariables = [
  "OPENAI_BASE_URL",
  "FLAGGO_ANALYSIS_BYOK_WIRE_API",
  "OPENAI_API_KEY",
];

export function analysisInferenceEnvironment(environment = process.env) {
  const mode = environment.FLAGGO_ANALYSIS_INFERENCE ?? "github";
  if (mode !== "github" && mode !== "byok") {
    throw new Error("FLAGGO_ANALYSIS_INFERENCE must be github or byok.");
  }
  const result = { FLAGGO_ANALYSIS_INFERENCE: mode };
  if (environment.FLAGGO_ANALYSIS_MODEL !== undefined) {
    requireValue(environment, "FLAGGO_ANALYSIS_MODEL");
    result.FLAGGO_ANALYSIS_MODEL = environment.FLAGGO_ANALYSIS_MODEL;
  }
  if (mode === "github") {
    if (environment.FLAGGO_ANALYSIS_BYOK_WIRE_API !== undefined) {
      throw new Error("FLAGGO_ANALYSIS_BYOK_WIRE_API requires FLAGGO_ANALYSIS_INFERENCE=byok.");
    }
    const token = environment.GH_TOKEN ?? environment.GITHUB_TOKEN;
    if (typeof token !== "string" || token.trim() === "") {
      throw new Error("Copilot analysis requires GH_TOKEN or GITHUB_TOKEN.");
    }
    result.GH_TOKEN = token;
    return result;
  }
  requireValue(environment, "FLAGGO_ANALYSIS_MODEL");
  if (!["responses", "completions"].includes(environment.FLAGGO_ANALYSIS_BYOK_WIRE_API)) {
    throw new Error("FLAGGO_ANALYSIS_BYOK_WIRE_API must be responses or completions.");
  }
  let endpoint;
  try {
    endpoint = new URL(requireValue(environment, "OPENAI_BASE_URL"));
  } catch {
    throw new Error("OPENAI_BASE_URL must be an absolute HTTP(S) URL.");
  }
  const loopback = endpoint.hostname === "localhost" || endpoint.hostname === "[::1]"
    || /^127(?:\.\d{1,3}){3}$/u.test(endpoint.hostname);
  if (endpoint.username || endpoint.password || endpoint.href.includes("?") || endpoint.href.includes("#")
    || !(endpoint.protocol === "https:" || endpoint.protocol === "http:" && loopback)) {
    throw new Error("OPENAI_BASE_URL requires HTTPS (HTTP only for loopback), without URL credentials, query or fragment.");
  }
  if (!loopback || environment.OPENAI_API_KEY !== undefined) {
    requireValue(environment, "OPENAI_API_KEY");
  }
  for (const name of byokVariables) {
    if (environment[name] !== undefined) result[name] = environment[name];
  }
  return result;
}

function requireValue(environment, name) {
  const value = environment[name];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${name} is required and must not be empty.`);
  }
  return value;
}
