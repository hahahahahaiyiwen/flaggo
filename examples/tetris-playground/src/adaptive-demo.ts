import { adaptiveDemoProtocol } from "../../tetris-integration/dist/flaggo/adaptive-protocol.js";
import type { PolicyEvaluation } from "./types.js";

export function assertExperienceImprovement(
  baseline: PolicyEvaluation,
  learned: PolicyEvaluation,
): void {
  for (const result of [baseline, learned]) {
    const measures = [
      result.episodes, result.plannedPlayMs, result.activePlayMs,
      result.challengeBalance, result.linesCleared, result.linesPerMinute, result.gameOvers,
    ];
    if (measures.some((value) => !Number.isFinite(value))
      || result.episodes <= 0 || result.activePlayMs <= 0
      || result.activePlayMs > result.plannedPlayMs
      || result.challengeBalance < 0 || result.challengeBalance > 1
      || result.linesPerMinute < 0 || result.linesCleared < 0
      || result.gameOvers < 0 || result.gameOvers > result.episodes) {
      throw new Error("Evaluation requires bounded finite gameplay measurements.");
    }
  }
  if (baseline.episodes !== learned.episodes
    || baseline.plannedPlayMs !== learned.plannedPlayMs) {
    throw new Error("Evaluation policies must use the same episode budget.");
  }
  const gain = learned.challengeBalance - baseline.challengeBalance;
  if (gain + Number.EPSILON < adaptiveDemoProtocol.minimumBalanceImprovement) {
    throw new Error(
      `Inconclusive comparison: balance improved ${(gain * 100).toFixed(1)} percentage points; `
      + "at least 5 are required.",
    );
  }
  if (learned.linesPerMinute
    < baseline.linesPerMinute * adaptiveDemoProtocol.minimumLineRetention) {
    throw new Error("Learned throughput fell below 90% of matched baseline throughput.");
  }
  if (learned.gameOvers > baseline.gameOvers
    || learned.activePlayMs < baseline.activePlayMs) {
    throw new Error("Learned gameplay produced more or earlier game overs.");
  }
}
