export function parseIntegrationArguments(arguments_) {
  let analysisMode;
  for (const argument of arguments_) {
    if (argument === "--analysis" || argument === "--deterministic-analysis") {
      const selected = argument === "--analysis" ? "copilot" : "deterministic";
      if (analysisMode !== undefined && analysisMode !== selected) {
        throw new Error("Select either --analysis or --deterministic-analysis, not both.");
      }
      analysisMode = selected;
    } else {
      throw new Error(`Unsupported Tetris integration argument '${argument}'.`);
    }
  }
  return { analysisMode };
}
