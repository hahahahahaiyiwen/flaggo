import {
  boardHeight,
  boardWidth,
  enumerateTetrisPlacements,
  type TetrisPlacement,
  type TetrisPlacementCommand,
  type TetrisPlanningState,
} from "./game.js";
import type { TetrisSessionApi } from "./session.js";

export interface GreedyPlayerPacing {
  readonly reactionDelayMs: number;
  readonly commandIntervalMs: number;
}

export function chooseGreedyPlacement(state: TetrisPlanningState): TetrisPlacement {
  const placements = enumerateTetrisPlacements(state);
  let best: TetrisPlacement | undefined;
  let bestScore = -Infinity;
  for (const placement of placements) {
    const score = placementScore(placement);
    if (
      score > bestScore
      || (score === bestScore && best !== undefined
        && placement.commands.length < best.commands.length)
    ) {
      best = placement;
      bestScore = score;
    }
  }
  if (best === undefined) throw new Error("The current piece has no legal placement.");
  return best;
}

function placementScore(placement: TetrisPlacement): number {
  const heights: number[] = [];
  let holes = 0;
  for (let x = 0; x < boardWidth; x += 1) {
    let occupied = false;
    let height = 0;
    for (let y = 0; y < boardHeight; y += 1) {
      if (placement.board[y]![x] !== null) {
        if (!occupied) height = boardHeight - y;
        occupied = true;
      } else if (occupied) {
        holes += 1;
      }
    }
    heights.push(height);
  }
  const aggregateHeight = heights.reduce((total, height) => total + height, 0);
  const bumpiness = heights.slice(1).reduce(
    (total, height, index) => total + Math.abs(height - heights[index]!),
    0,
  );
  return placement.linesCleared * 3
    - aggregateHeight * 0.5
    - holes * 3
    - bumpiness * 0.25;
}

export class GreedyTetrisPlayer {
  private pieceSequence: number | undefined;
  private actionAt: number | undefined;
  private lastObservedAt = -Infinity;

  constructor(
    private readonly session: TetrisSessionApi,
    private readonly pacing: GreedyPlayerPacing,
  ) {
    if (!Number.isFinite(pacing.reactionDelayMs) || pacing.reactionDelayMs < 0) {
      throw new RangeError("Player reaction delay must be finite and non-negative.");
    }
    if (!Number.isFinite(pacing.commandIntervalMs) || pacing.commandIntervalMs <= 0) {
      throw new RangeError("Player command interval must be finite and positive.");
    }
  }

  reset(): void {
    this.pieceSequence = undefined;
    this.actionAt = undefined;
  }

  nextActionAt(now: number): number | undefined {
    if (!Number.isFinite(now) || now < this.lastObservedAt) {
      throw new RangeError("Player timestamps must be finite and chronological.");
    }
    this.lastObservedAt = now;
    if (this.session.snapshot().status !== "playing") {
      this.reset();
      return undefined;
    }
    const sequence = this.session.planningState().pieceSequence;
    if (sequence !== this.pieceSequence) {
      this.pieceSequence = sequence;
      this.actionAt = Math.max(now + this.pacing.reactionDelayMs, this.actionAt ?? now);
    }
    return this.actionAt;
  }

  nextCommand(now: number): TetrisPlacementCommand | undefined {
    const actionAt = this.nextActionAt(now);
    if (actionAt === undefined || now < actionAt) return undefined;
    const command = chooseGreedyPlacement(this.session.planningState()).commands[0];
    if (command === undefined) throw new Error("The greedy placement plan is empty.");
    this.actionAt = now + this.pacing.commandIntervalMs;
    return command;
  }
}
