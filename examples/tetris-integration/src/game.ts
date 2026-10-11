import type { DropIntervalObservation } from "./drop-interval.js";

export const boardWidth = 10;
export const boardHeight = 20;

export type TetrominoKind = "I" | "J" | "L" | "O" | "S" | "T" | "Z";
export type BoardCell = TetrominoKind | null;
export type Board = readonly (readonly BoardCell[])[];

interface Coordinate {
  readonly x: number;
  readonly y: number;
}

export interface TetrisPiecePose {
  readonly kind: TetrominoKind;
  readonly rotation: number;
  readonly x: number;
  readonly y: number;
}

export type TetrisPlacementCommand =
  | "move-left"
  | "move-right"
  | "rotate-clockwise"
  | "hard-drop";

export interface TetrisPlanningState {
  readonly board: Board;
  readonly activePiece: Readonly<TetrisPiecePose>;
  readonly pieceSequence: number;
}

export interface TetrisPlacement {
  readonly commands: readonly TetrisPlacementCommand[];
  readonly board: Board;
  readonly linesCleared: number;
}

export interface PieceSource {
  next(): TetrominoKind;
}

export interface TetrisGameOptions {
  readonly pieceSource?: PieceSource;
  readonly now?: () => number;
  readonly sessionId?: string;
  readonly startedAt?: number;
}

export interface GameUpdate {
  readonly changed: boolean;
  readonly dropDistance: number;
  readonly locked: boolean;
  readonly lockedPiece?: TetrominoKind;
  readonly linesCleared: number;
  readonly placementTimeMs?: number;
  readonly recoveryFailureRecorded: boolean;
  readonly spawnedPiece?: TetrominoKind;
  readonly gameOver: boolean;
}

export interface GameSnapshot {
  readonly board: Board;
  readonly activePiece: TetrominoKind;
  readonly score: number;
  readonly lines: number;
  readonly level: number;
  readonly paused: boolean;
  readonly gameOver: boolean;
  readonly pendingRecoveryFailures: number;
  readonly dropObservation: DropIntervalObservation;
}

const kinds: readonly TetrominoKind[] = [
  "I",
  "J",
  "L",
  "O",
  "S",
  "T",
  "Z",
];

const rotations: Readonly<Record<
  TetrominoKind,
  readonly (readonly Coordinate[])[]
>> = {
  I: [
    [{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 3, y: 1 }],
    [{ x: 2, y: 0 }, { x: 2, y: 1 }, { x: 2, y: 2 }, { x: 2, y: 3 }],
    [{ x: 0, y: 2 }, { x: 1, y: 2 }, { x: 2, y: 2 }, { x: 3, y: 2 }],
    [{ x: 1, y: 0 }, { x: 1, y: 1 }, { x: 1, y: 2 }, { x: 1, y: 3 }],
  ],
  J: [
    [{ x: 0, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 1, y: 1 }, { x: 1, y: 2 }],
    [{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 2, y: 2 }],
    [{ x: 1, y: 0 }, { x: 1, y: 1 }, { x: 0, y: 2 }, { x: 1, y: 2 }],
  ],
  L: [
    [{ x: 2, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 1, y: 1 }, { x: 1, y: 2 }, { x: 2, y: 2 }],
    [{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 0, y: 2 }],
    [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }, { x: 1, y: 2 }],
  ],
  O: [
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
  ],
  S: [
    [{ x: 1, y: 0 }, { x: 2, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }],
    [{ x: 1, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 2, y: 2 }],
    [{ x: 1, y: 1 }, { x: 2, y: 1 }, { x: 0, y: 2 }, { x: 1, y: 2 }],
    [{ x: 0, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 1, y: 2 }],
  ],
  T: [
    [{ x: 1, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 1, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 1, y: 2 }],
    [{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 1, y: 2 }],
    [{ x: 1, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 1, y: 2 }],
  ],
  Z: [
    [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }],
    [{ x: 2, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 1 }, { x: 1, y: 2 }],
    [{ x: 0, y: 1 }, { x: 1, y: 1 }, { x: 1, y: 2 }, { x: 2, y: 2 }],
    [{ x: 1, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }, { x: 0, y: 2 }],
  ],
};

const lineScores = [0, 100, 300, 500, 800] as const;

export class SevenBagPieceSource implements PieceSource {
  private bag: TetrominoKind[] = [];

  constructor(private readonly random: () => number = Math.random) {}

  next(): TetrominoKind {
    if (this.bag.length === 0) this.refill();
    return this.bag.pop()!;
  }

  private refill(): void {
    this.bag = [...kinds];
    for (let index = this.bag.length - 1; index > 0; index -= 1) {
      const randomValue = this.random();
      if (
        !Number.isFinite(randomValue)
        || randomValue < 0
        || randomValue >= 1
      ) {
        throw new RangeError("Tetris random sources must return values in [0, 1).");
      }
      const swapIndex = Math.floor(randomValue * (index + 1));
      [this.bag[index], this.bag[swapIndex]] = [
        this.bag[swapIndex]!,
        this.bag[index]!,
      ];
    }
  }
}

export class SequencePieceSource implements PieceSource {
  private index = 0;

  constructor(private readonly sequence: readonly TetrominoKind[]) {
    if (sequence.length === 0) {
      throw new RangeError("A piece sequence cannot be empty.");
    }
  }

  next(): TetrominoKind {
    const value = this.sequence[this.index % this.sequence.length]!;
    this.index += 1;
    return value;
  }
}

function emptyBoard(): BoardCell[][] {
  return Array.from(
    { length: boardHeight },
    () => Array<BoardCell>(boardWidth).fill(null),
  );
}

function cells(piece: Readonly<TetrisPiecePose>): readonly Coordinate[] {
  return rotations[piece.kind][piece.rotation]!.map((cell) => ({
    x: piece.x + cell.x,
    y: piece.y + cell.y,
  }));
}

function collidesWithBoard(
  board: Board,
  piece: Readonly<TetrisPiecePose>,
): boolean {
  return cells(piece).some((cell) =>
    cell.x < 0
    || cell.x >= boardWidth
    || cell.y >= boardHeight
    || (cell.y >= 0 && board[cell.y]![cell.x] !== null));
}

function rotatedPiece(
  board: Board,
  piece: Readonly<TetrisPiecePose>,
): TetrisPiecePose | undefined {
  const rotation = (piece.rotation + 1) % 4;
  for (const offset of [0, -1, 1, -2, 2]) {
    const candidate = { ...piece, rotation, x: piece.x + offset };
    if (!collidesWithBoard(board, candidate)) return candidate;
  }
  return undefined;
}

function clearedBoard(board: Board): { board: BoardCell[][]; lines: number } {
  const remaining = board.filter((row) => row.some((cell) => cell === null));
  const lines = boardHeight - remaining.length;
  return {
    board: [
      ...Array.from({ length: lines }, () => Array<BoardCell>(boardWidth).fill(null)),
      ...remaining.map((row) => [...row]),
    ],
    lines,
  };
}

export function enumerateTetrisPlacements(
  state: TetrisPlanningState,
): readonly TetrisPlacement[] {
  const queue: Array<{
    piece: Readonly<TetrisPiecePose>;
    commands: readonly TetrisPlacementCommand[];
  }> = [{ piece: state.activePiece, commands: [] }];
  const visited = new Set<string>();
  const placements: TetrisPlacement[] = [];
  for (let index = 0; index < queue.length; index += 1) {
    const { piece, commands } = queue[index]!;
    const key = `${piece.x}:${piece.y}:${piece.rotation}`;
    if (visited.has(key) || collidesWithBoard(state.board, piece)) continue;
    visited.add(key);
    let landing = { ...piece };
    while (!collidesWithBoard(state.board, { ...landing, y: landing.y + 1 })) {
      landing = { ...landing, y: landing.y + 1 };
    }
    const projected = state.board.map((row) => [...row]);
    for (const cell of cells(landing)) {
      if (cell.y < 0) throw new Error("A planned placement extends above the board.");
      projected[cell.y]![cell.x] = landing.kind;
    }
    const cleared = clearedBoard(projected);
    placements.push({
      commands: [...commands, "hard-drop"],
      board: cleared.board,
      linesCleared: cleared.lines,
    });
    for (const [command, candidate] of [
      ["move-left", { ...piece, x: piece.x - 1 }],
      ["move-right", { ...piece, x: piece.x + 1 }],
      ["rotate-clockwise", rotatedPiece(state.board, piece)],
    ] as const) {
      if (candidate !== undefined && !collidesWithBoard(state.board, candidate)) {
        queue.push({ piece: candidate, commands: [...commands, command] });
      }
    }
  }
  return placements;
}

export class TetrisGame {
  private readonly board = emptyBoard();
  private readonly pieceSource: PieceSource;
  private readonly now: () => number;
  private readonly sessionId: string;
  private active: TetrisPiecePose;
  private pieceSequence = 0;
  private spawnedAt: number;
  private currentRecoveryFailures = 0;
  private recentPlacementTimeMs = 0;
  private recentRecoveryFailures = 0;
  private scoreValue = 0;
  private lineCount = 0;
  private pausedValue = false;
  private gameOverValue = false;

  constructor(options: TetrisGameOptions = {}) {
    this.pieceSource = options.pieceSource ?? new SevenBagPieceSource();
    this.now = options.now ?? Date.now;
    this.sessionId = options.sessionId
      ?? `tetris-${Math.floor(this.now()).toString(36)}`;
    this.spawnedAt = options.startedAt ?? this.now();
    assertTimestamp(this.spawnedAt);
    this.active = this.newPiece(this.pieceSource.next());
    if (this.collides(this.active)) this.gameOverValue = true;
  }

  get score(): number {
    return this.scoreValue;
  }

  get lines(): number {
    return this.lineCount;
  }

  get level(): number {
    return Math.min(20, Math.floor(this.lineCount / 10));
  }

  get paused(): boolean {
    return this.pausedValue;
  }

  get gameOver(): boolean {
    return this.gameOverValue;
  }

  setPaused(paused: boolean): void {
    if (!this.gameOverValue) this.pausedValue = paused;
  }

  moveLeft(): GameUpdate {
    return this.move(-1, 0);
  }

  moveRight(): GameUpdate {
    return this.move(1, 0);
  }

  rotateClockwise(): GameUpdate {
    if (!this.canAct()) return this.update(false);
    const candidate = rotatedPiece(this.board, this.active);
    if (candidate !== undefined) {
      this.active = candidate;
      return this.update(true);
    }
    this.currentRecoveryFailures += 1;
    return this.update(false, { recoveryFailureRecorded: true });
  }

  softDrop(now = this.now()): GameUpdate {
    if (!this.canAct()) return this.update(false);
    assertTimestamp(now);
    const candidate = { ...this.active, y: this.active.y + 1 };
    if (!this.collides(candidate)) {
      this.active = candidate;
      this.scoreValue += 1;
      return this.update(true, { dropDistance: 1 });
    }
    return this.lock(now);
  }

  hardDrop(now = this.now()): GameUpdate {
    if (!this.canAct()) return this.update(false);
    assertTimestamp(now);
    let distance = 0;
    while (true) {
      const candidate = { ...this.active, y: this.active.y + 1 };
      if (this.collides(candidate)) break;
      this.active = candidate;
      distance += 1;
    }
    this.scoreValue += distance * 2;
    return this.lock(now, distance);
  }

  tick(now = this.now()): GameUpdate {
    if (!this.canAct()) return this.update(false);
    assertTimestamp(now);
    const candidate = { ...this.active, y: this.active.y + 1 };
    if (!this.collides(candidate)) {
      this.active = candidate;
      return this.update(true, { dropDistance: 1 });
    }
    return this.lock(now);
  }

  snapshot(): GameSnapshot {
    const visible = this.board.map((row) => [...row]);
    if (!this.gameOverValue) {
      for (const cell of cells(this.active)) {
        if (
          cell.y >= 0
          && cell.y < boardHeight
          && cell.x >= 0
          && cell.x < boardWidth
        ) {
          visible[cell.y]![cell.x] = this.active.kind;
        }
      }
    }
    return {
      board: visible,
      activePiece: this.active.kind,
      score: this.scoreValue,
      lines: this.lineCount,
      level: this.level,
      paused: this.pausedValue,
      gameOver: this.gameOverValue,
      pendingRecoveryFailures: this.currentRecoveryFailures,
      dropObservation: {
        boardPressure: this.boardPressure(),
        currentLevel: this.level,
        placementTimeMs: this.recentPlacementTimeMs,
        recoveryFailures: this.recentRecoveryFailures,
        sessionId: this.sessionId,
      },
    };
  }

  planningState(): TetrisPlanningState {
    return {
      board: this.board.map((row) => [...row]),
      activePiece: { ...this.active },
      pieceSequence: this.pieceSequence,
    };
  }

  private canAct(): boolean {
    return !this.pausedValue && !this.gameOverValue;
  }

  private move(x: number, y: number): GameUpdate {
    if (!this.canAct()) return this.update(false);
    const candidate = {
      ...this.active,
      x: this.active.x + x,
      y: this.active.y + y,
    };
    if (this.collides(candidate)) {
      this.currentRecoveryFailures += 1;
      return this.update(false, { recoveryFailureRecorded: true });
    }
    this.active = candidate;
    return this.update(true);
  }

  private lock(now: number, dropDistance = 0): GameUpdate {
    const lockedPiece = this.active.kind;
    const placementTimeMs = Math.max(0, now - this.spawnedAt);
    for (const cell of cells(this.active)) {
      if (cell.y < 0) {
        this.gameOverValue = true;
        return this.update(true, {
          dropDistance,
          locked: true,
          lockedPiece,
          placementTimeMs,
        });
      }
      this.board[cell.y]![cell.x] = this.active.kind;
    }

    const linesCleared = this.clearLines();
    this.scoreValue += lineScores[linesCleared]! * (this.level + 1);
    this.recentPlacementTimeMs = placementTimeMs;
    this.recentRecoveryFailures = Math.min(
      5,
      this.currentRecoveryFailures,
    );
    this.currentRecoveryFailures = 0;
    this.active = this.newPiece(this.pieceSource.next());
    this.pieceSequence += 1;
    this.spawnedAt = now;
    if (this.collides(this.active)) this.gameOverValue = true;
    return this.update(true, {
      dropDistance,
      locked: true,
      lockedPiece,
      linesCleared,
      placementTimeMs,
      ...(this.gameOverValue ? {} : { spawnedPiece: this.active.kind }),
    });
  }

  private clearLines(): number {
    const cleared = clearedBoard(this.board);
    this.board.splice(0, this.board.length, ...cleared.board);
    this.lineCount += cleared.lines;
    return cleared.lines;
  }

  private collides(piece: TetrisPiecePose): boolean {
    return collidesWithBoard(this.board, piece);
  }

  private newPiece(kind: TetrominoKind): TetrisPiecePose {
    return {
      kind,
      rotation: 0,
      x: Math.floor((boardWidth - 4) / 2),
      y: 0,
    };
  }

  private boardPressure(): number {
    let highestOccupiedRow = boardHeight;
    for (let y = 0; y < boardHeight; y += 1) {
      if (this.board[y]!.some((cell) => cell !== null)) {
        highestOccupiedRow = y;
        break;
      }
    }
    return (boardHeight - highestOccupiedRow) / boardHeight;
  }

  private update(
    changed: boolean,
    details: Partial<Omit<GameUpdate, "changed" | "gameOver">> = {},
  ): GameUpdate {
    return {
      changed,
      dropDistance: details.dropDistance ?? 0,
      locked: details.locked ?? false,
      ...(details.lockedPiece === undefined
        ? {}
        : { lockedPiece: details.lockedPiece }),
      linesCleared: details.linesCleared ?? 0,
      ...(details.placementTimeMs === undefined
        ? {}
        : { placementTimeMs: details.placementTimeMs }),
      recoveryFailureRecorded: details.recoveryFailureRecorded ?? false,
      ...(details.spawnedPiece === undefined
        ? {}
        : { spawnedPiece: details.spawnedPiece }),
      gameOver: this.gameOverValue,
    };
  }
}

function assertTimestamp(value: number): void {
  if (!Number.isFinite(value)) {
    throw new RangeError("Tetris game timestamps must be finite.");
  }
}
