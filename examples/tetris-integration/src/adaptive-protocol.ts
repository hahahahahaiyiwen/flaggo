import { SevenBagPieceSource } from "./game.js";

export const adaptiveDemoProtocol = {
  durationMs: 120_000,
  reactionDelayMs: 3_000,
  commandIntervalMs: 200,
  trainingSeeds: [11, 29, 53, 17, 37, 61, 13, 41, 67],
  evaluationSeeds: [71, 89, 107],
  minimumBalanceImprovement: 0.05,
  minimumLineRetention: 0.9,
} as const;

export function seededRandom(seed: number): () => number {
  let value = seed >>> 0;
  return () => {
    value = (Math.imul(value, 1_664_525) + 1_013_904_223) >>> 0;
    return value / 4_294_967_296;
  };
}

export function scenarioPieceSource(seed: number): SevenBagPieceSource {
  return new SevenBagPieceSource(seededRandom(seed));
}
