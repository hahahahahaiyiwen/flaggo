import { createHash, randomUUID } from "node:crypto";
import type { DropIntervalProvider } from "../../tetris-integration/dist/flaggo/drop-interval.js";
import {
  TetrisSession,
  type TetrisSessionInstrumentation,
  type TetrisSessionState,
} from "../../tetris-integration/dist/flaggo/session.js";
import {
  TetrisExperience,
  type ExperienceTotals,
  type ExperienceWindow,
} from "../../tetris-integration/dist/flaggo/experience.js";
import {
  adaptiveDemoProtocol,
  scenarioPieceSource,
  seededRandom,
} from "../../tetris-integration/dist/flaggo/adaptive-protocol.js";
import { runRealtimeAutonomousTetrisSession } from "../../tetris-integration/dist/flaggo/realtime-autonomous-session.js";
import { PlaygroundRequestError } from "./errors.js";
import type {
  BotSlotView,
  DecisionView,
  PlaygroundMode,
  HumanSessionView,
  TetrisSessionCommand,
} from "./types.js";

export interface SessionProviderContext {
  readonly experience: TetrisExperience;
  readonly state: () => TetrisSessionState;
  readonly now: () => number;
  readonly random: () => number;
  readonly learning: boolean;
  readonly onDecision?: (decision: DecisionView) => void;
}

interface PopulationOptions {
  readonly mode: PlaygroundMode;
  readonly createProvider: (options: SessionProviderContext) => DropIntervalProvider;
  readonly onChanged: () => void;
  readonly recordExperience: (window: ExperienceWindow) => void;
  readonly instrumentation?: TetrisSessionInstrumentation;
  readonly now?: () => number;
  readonly episodeDurationMs?: number;
}

interface Episode {
  readonly experience: TetrisExperience;
  state?: TetrisSessionState;
  decision?: DecisionView;
  at: number;
  counted: boolean;
}

interface BotSlot {
  readonly id: string;
  readonly nextSeed: () => number;
  status: BotSlotView["status"];
  episode: number;
  seed?: number;
  record?: Episode;
  totals: ExperienceTotals;
  controller?: AbortController;
  task?: Promise<void>;
  error?: string;
}

interface Human {
  readonly session: TetrisSession;
  readonly experience: TetrisExperience;
  readonly now: () => number;
  decision?: DecisionView;
  error?: string;
}

export class TetrisPopulation {
  private readonly clock: () => number;
  private readonly slots: BotSlot[];
  private human: Human;
  private humanTotals = emptyTotals();
  private gravityTimer: NodeJS.Timeout | undefined;
  private gravityInterval = 800;
  private policyTimer: NodeJS.Timeout | undefined;
  private humanRefresh: Promise<void> | undefined;
  private closed = false;

  constructor(private readonly options: PopulationOptions) {
    this.clock = options.now ?? (() => performance.now());
    this.slots = options.mode === "benchmark" ? [] : [
      211, 223, 227, 229, 233, 239, 241, 251, 257, 263,
    ].map((seed, index) => ({
      id: `bot-${index + 1}`,
      nextSeed: seededRandom(seed),
      status: "idle",
      episode: 0,
      totals: emptyTotals(),
    }));
    this.human = this.createHuman();
  }

  start(): void {
    this.scheduleGravity();
    this.policyTimer = setInterval(() => this.requestHumanPolicy(), 5_000);
    this.requestHumanPolicy();
  }

  snapshot(): { readonly human: HumanSessionView; readonly bots: readonly BotSlotView[] } {
    return {
      human: {
        state: this.human.session.snapshot(),
        experience: this.human.experience.totals(),
        totals: addTotals(this.humanTotals, this.human.experience.totals()),
        ...(this.human.decision === undefined ? {} : { decision: this.human.decision }),
        ...(this.human.error === undefined ? {} : { error: this.human.error }),
      },
      bots: this.slots.map((slot) => ({
        id: slot.id,
        status: slot.status,
        episode: slot.episode,
        ...(slot.seed === undefined ? {} : { seed: slot.seed }),
        ...(slot.error === undefined ? {} : { error: slot.error }),
        totals: addTotals(slot.totals,
          slot.record === undefined || slot.record.counted ? emptyTotals() : slot.record.experience.totals()),
        ...(slot.record?.state === undefined ? {} : {
          session: {
            state: slot.record.state,
            experience: slot.record.experience.totals(),
            ...(slot.record.decision === undefined ? {} : { decision: slot.record.decision }),
          },
        }),
      })),
    };
  }

  command(command: TetrisSessionCommand): void {
    if (this.closed) throw new PlaygroundRequestError(409, "The playground is closed.");
    let locked = false;
    if (command === "restart") {
      this.human.session.close();
      this.human.experience.observe(this.human.session.snapshot(), this.human.now());
      this.humanTotals = addTotals(this.humanTotals, this.human.experience.totals());
      this.human = this.createHuman();
    } else {
      const transition = this.human.session.dispatch(command);
      locked = transition.update?.locked === true;
      this.human.experience.observe(transition.state, this.human.now(), transition);
    }
    if (locked || command === "restart" || command === "pause" || command === "resume") {
      this.resetGravity();
    }
    this.options.onChanged();
    if (command === "restart" || command === "resume") this.requestHumanPolicy();
  }

  startBot(id: string): void {
    if (this.closed) throw new PlaygroundRequestError(409, "The playground is closed.");
    const slot = this.slot(id);
    if (slot.status === "running" || slot.status === "stopping") {
      throw new PlaygroundRequestError(409, `${id} is already running or stopping.`);
    }
    delete slot.error;
    slot.status = "running";
    const controller = new AbortController();
    slot.controller = controller;
    slot.task = this.runBot(slot, controller).finally(() => {
      delete slot.task;
      delete slot.controller;
      if (slot.status !== "failed") slot.status = "idle";
      this.options.onChanged();
    });
    this.options.onChanged();
  }

  startAll(): void {
    for (const slot of this.slots) {
      if (slot.status === "idle") this.startBot(slot.id);
    }
  }

  async stopBot(id: string): Promise<void> {
    const slot = this.slot(id);
    if (slot.task === undefined) return;
    slot.status = "stopping";
    slot.controller?.abort(new Error(`${id} stopped by the user.`));
    this.options.onChanged();
    await slot.task;
  }

  async stopAll(): Promise<void> {
    await Promise.all(this.slots.map((slot) => this.stopBot(slot.id)));
  }

  async refreshHumanPolicy(): Promise<void> {
    const human = this.human;
    if (this.closed || human.session.snapshot().status !== "playing") return;
    const transition = await human.session.refreshPolicy();
    if (this.closed || human !== this.human) return;
    human.experience.observe(transition.state, human.now(), transition);
    if (transition.state.dropInterval.intervalMs !== this.gravityInterval) this.resetGravity();
    this.options.onChanged();
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (this.gravityTimer !== undefined) clearTimeout(this.gravityTimer);
    if (this.policyTimer !== undefined) clearInterval(this.policyTimer);
    this.human.session.close();
    this.human.experience.observe(this.human.session.snapshot(), this.human.now());
    await Promise.all([this.stopAll(), this.humanRefresh]);
  }

  private createHuman(): Human {
    const startedAt = this.clock();
    const now = (): number => this.clock() - startedAt;
    const sessionId = `tetris-human-${randomUUID()}`;
    const randomValue = sessionRandom(sessionId);
    const learning = this.options.mode === "live";
    const experience = new TetrisExperience({
      dataset: learning ? "live" : "evaluation", origin: "human", clock: "wall",
    }, learning ? this.options.recordExperience : undefined);
    let session: TetrisSession | undefined;
    const provider = this.options.createProvider({
      experience,
      state: () => {
        if (session === undefined) throw new Error("Observe the human session before deciding.");
        return session.snapshot();
      },
      now,
      random: () => randomValue,
      learning,
      onDecision: (decision) => {
        if (this.human?.session === session) this.human.decision = decision;
      },
    });
    session = new TetrisSession({
      provider,
      ...(this.options.instrumentation === undefined ? {} : { instrumentation: this.options.instrumentation }),
      game: { sessionId },
      fallbackIntervalMs: 800,
      policyApplication: "piece-boundary",
      now,
    });
    experience.observe(session.snapshot(), now());
    return { session, experience, now };
  }

  private slot(id: string): BotSlot {
    const slot = this.slots.find((value) => value.id === id);
    if (slot === undefined) throw new PlaygroundRequestError(404, `Unknown bot slot '${id}'.`);
    return slot;
  }

  private async runBot(slot: BotSlot, controller: AbortController): Promise<void> {
    try {
      while (!controller.signal.aborted) {
        slot.seed = Math.floor(slot.nextSeed() * 4_294_967_296);
        slot.episode += 1;
        const sessionId = `tetris-${slot.id}-${randomUUID()}`;
        const randomValue = sessionRandom(sessionId);
        const record: Episode = {
          experience: new TetrisExperience(
            { dataset: "live", origin: "greedy", clock: "wall" }, this.options.recordExperience),
          at: 0,
          counted: false,
        };
        slot.record = record;
        const provider = this.options.createProvider({
          experience: record.experience,
          state: () => {
            if (record.state === undefined) throw new Error("Observe the bot before deciding.");
            return record.state;
          },
          now: () => record.at,
          random: () => randomValue,
          learning: true,
          onDecision: (decision) => {
            if (slot.record === record && slot.status === "running") record.decision = decision;
          },
        });
        try {
          const result = await runRealtimeAutonomousTetrisSession({
            durationMs: this.options.episodeDurationMs ?? adaptiveDemoProtocol.durationMs,
            reactionDelayMs: adaptiveDemoProtocol.reactionDelayMs,
            commandIntervalMs: adaptiveDemoProtocol.commandIntervalMs,
            sessionId,
            pieceSource: scenarioPieceSource(slot.seed),
            provider,
            signal: controller.signal,
            now: this.clock,
            ...(this.options.instrumentation === undefined ? {} : { instrumentation: this.options.instrumentation }),
            onState: (state, at, transition) => {
              if (slot.record !== record) return;
              record.state = state;
              record.at = at;
              record.experience.observe(state, at, transition);
              this.options.onChanged();
            },
          });
          if (result.state.dropInterval.exposureId !== undefined) record.experience.completeEpisode();
        } finally {
          slot.totals = addTotals(slot.totals, record.experience.totals());
          record.counted = true;
        }
      }
    } catch (error) {
      if (!controller.signal.aborted || error !== controller.signal.reason) {
        slot.status = "failed";
        slot.error = error instanceof Error ? error.message : String(error);
      }
    }
  }

  private requestHumanPolicy(): void {
    const human = this.human;
    const task = this.refreshHumanPolicy().catch((error: unknown) => {
      if (this.closed || human !== this.human) return;
      human.error = error instanceof Error ? error.message : String(error);
      human.session.close();
      if (this.gravityTimer !== undefined) clearTimeout(this.gravityTimer);
      this.options.onChanged();
    }).finally(() => {
      if (this.humanRefresh === task) this.humanRefresh = undefined;
    });
    this.humanRefresh = task;
  }

  private scheduleGravity(): void {
    if (this.closed || this.human.session.snapshot().status !== "playing") return;
    this.gravityInterval = this.human.session.snapshot().dropInterval.intervalMs;
    this.gravityTimer = setTimeout(() => {
      this.gravityTimer = undefined;
      const transition = this.human.session.dispatch("gravity-tick");
      this.human.experience.observe(transition.state, this.human.now(), transition);
      this.options.onChanged();
      this.scheduleGravity();
    }, this.gravityInterval);
  }

  private resetGravity(): void {
    if (this.gravityTimer !== undefined) clearTimeout(this.gravityTimer);
    this.gravityTimer = undefined;
    this.scheduleGravity();
  }
}

function sessionRandom(sessionId: string): number {
  const digest = createHash("sha256").update(sessionId).digest();
  return digest.readUInt32BE(0) / 2 ** 32;
}

function emptyTotals(): ExperienceTotals {
  return { activePlayMs: 0, balancedPlayMs: 0, linesCleared: 0, gameOvers: 0 };
}

function addTotals(left: ExperienceTotals, right: ExperienceTotals): ExperienceTotals {
  return {
    activePlayMs: left.activePlayMs + right.activePlayMs,
    balancedPlayMs: left.balancedPlayMs + right.balancedPlayMs,
    linesCleared: left.linesCleared + right.linesCleared,
    gameOvers: left.gameOvers + right.gameOvers,
  };
}
