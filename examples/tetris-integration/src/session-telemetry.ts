import {
  context,
  SpanStatusCode,
  trace,
  type Attributes,
  type Context,
  type Counter,
  type Gauge,
  type Histogram,
  type Meter,
  type Span,
  type Tracer,
  type UpDownCounter,
} from "@opentelemetry/api";
import {
  SeverityNumber,
  type Logger,
} from "@opentelemetry/api-logs";

import type {
  TetrisSessionCommand,
  TetrisSessionCommandInstrumentation,
  TetrisSessionInstrumentation,
  TetrisSessionState,
  TetrisSessionTransition,
} from "./session.js";

export const tetrisEngineInstrumentationScope = "tetris.engine";

export interface TetrisSessionOpenTelemetry {
  readonly logger: Logger;
  readonly meter: Meter;
  readonly tracer: Tracer;
}

interface TetrisSessionMetrics {
  readonly activeSessions: UpDownCounter;
  readonly boardPressure: Gauge;
  readonly commands: Counter;
  readonly dropDistance: Histogram;
  readonly gamesCompleted: Counter;
  readonly gamesStarted: Counter;
  readonly level: Gauge;
  readonly linesCleared: Counter;
  readonly piecesLocked: Counter;
  readonly piecesSpawned: Counter;
  readonly placementTime: Histogram;
  readonly recoveryFailure: Counter;
  readonly score: Gauge;
}

export function createTetrisSessionInstrumentation(
  telemetry: TetrisSessionOpenTelemetry,
): TetrisSessionInstrumentation {
  return new OpenTelemetrySessionInstrumentation(telemetry);
}

class OpenTelemetrySessionInstrumentation
  implements TetrisSessionInstrumentation {
  private readonly metrics: TetrisSessionMetrics;

  constructor(private readonly telemetry: TetrisSessionOpenTelemetry) {
    this.metrics = createMetrics(telemetry.meter);
  }

  sessionStarted(state: TetrisSessionState): void {
    this.metrics.activeSessions.add(1);
    this.metrics.gamesStarted.add(1);
    this.metrics.piecesSpawned.add(1, {
      "tetris.piece.kind": state.activePiece,
    });
    this.recordState(state);
    emit(this.telemetry.logger, "tetris.game.started", stateAttributes(state));
    emit(this.telemetry.logger, "tetris.piece.spawned", {
      ...sessionAttributes(state),
      "tetris.piece.kind": state.activePiece,
    });
  }

  commandStarted(
    command: TetrisSessionCommand,
    state: TetrisSessionState,
  ): TetrisSessionCommandInstrumentation {
    const span = this.telemetry.tracer.startSpan("tetris.command", {
      attributes: {
        ...sessionAttributes(state),
        "tetris.command": command,
        "tetris.session.revision.start": state.revision,
      },
    });
    return new OpenTelemetryCommandInstrumentation(
      command,
      state,
      span,
      this.telemetry.logger,
      this.metrics,
      (next) => this.recordState(next),
    );
  }

  sessionClosed(_state: TetrisSessionState): void {
    this.metrics.activeSessions.add(-1);
  }

  private recordState(state: TetrisSessionState): void {
    const attributes = sessionAttributes(state);
    this.metrics.boardPressure.record(
      state.dropObservation.boardPressure,
      attributes,
    );
    this.metrics.score.record(state.score, attributes);
    this.metrics.level.record(state.level, attributes);
  }
}

class OpenTelemetryCommandInstrumentation
  implements TetrisSessionCommandInstrumentation {
  private readonly spanContext: Context;

  constructor(
    private readonly command: TetrisSessionCommand,
    private readonly before: TetrisSessionState,
    private readonly span: Span,
    private readonly logger: Logger,
    private readonly metrics: TetrisSessionMetrics,
    private readonly recordState: (state: TetrisSessionState) => void,
  ) {
    this.spanContext = trace.setSpan(context.active(), span);
  }

  completed(transition: TetrisSessionTransition): void {
    const outcome = commandOutcome(transition);
    this.metrics.commands.add(1, {
      "tetris.command": this.command,
      "tetris.command.outcome": outcome,
    });
    this.span.setAttributes({
      "tetris.command.outcome": outcome,
      "tetris.game.status": transition.state.status,
      "tetris.session.changed": transition.changed,
      "tetris.session.revision.end": transition.state.revision,
    });
    context.with(this.spanContext, () => this.recordTransition(transition));
    this.span.end();
  }

  failed(error: unknown): void {
    const failure = error instanceof Error ? error : new Error(String(error));
    this.metrics.commands.add(1, {
      "tetris.command": this.command,
      "tetris.command.outcome": "error",
    });
    this.span.recordException(failure);
    this.span.setAttributes({
      "tetris.command.outcome": "error",
      "error.type": failure.name,
    });
    this.span.setStatus({
      code: SpanStatusCode.ERROR,
      message: failure.message,
    });
    context.with(this.spanContext, () => {
      emit(this.logger, "tetris.command.failed", {
        ...sessionAttributes(this.before),
        "tetris.command": this.command,
        "error.type": failure.name,
        "error.message": failure.message,
      }, SeverityNumber.ERROR);
    });
    this.span.end();
  }

  private recordTransition(transition: TetrisSessionTransition): void {
    const state = transition.state;
    const update = transition.update;
    if (transition.changed) this.recordState(state);

    if (this.command === "restart") {
      this.metrics.gamesStarted.add(1);
      this.metrics.piecesSpawned.add(1, {
        "tetris.piece.kind": state.activePiece,
      });
      emit(this.logger, "tetris.game.restarted", stateAttributes(state));
      emit(this.logger, "tetris.piece.spawned", {
        ...sessionAttributes(state),
        "tetris.piece.kind": state.activePiece,
      });
    } else if (this.command === "pause" && transition.changed) {
      emit(this.logger, "tetris.game.paused", stateAttributes(state));
    } else if (this.command === "resume" && transition.changed) {
      emit(this.logger, "tetris.game.resumed", stateAttributes(state));
    }

    if (update === undefined) return;
    if (update.dropDistance > 0) {
      this.metrics.dropDistance.record(update.dropDistance, {
        "tetris.command": this.command,
      });
    }
    if (update.recoveryFailureRecorded) {
      const attributes = {
        ...sessionAttributes(state),
        "tetris.command": this.command,
      };
      this.metrics.recoveryFailure.add(1, sessionAttributes(state));
      this.span.addEvent("tetris.recovery.failed", attributes);
      emit(
        this.logger,
        "tetris.recovery.failed",
        attributes,
        SeverityNumber.WARN,
      );
    }
    if (update.locked && update.lockedPiece !== undefined) {
      const attributes = {
        ...sessionAttributes(state),
        "tetris.piece.kind": update.lockedPiece,
        "tetris.piece.drop_distance": update.dropDistance,
        "tetris.piece.placement_time_ms": update.placementTimeMs ?? 0,
        "tetris.board.pressure": state.dropObservation.boardPressure,
      };
      this.metrics.piecesLocked.add(1, {
        "tetris.piece.kind": update.lockedPiece,
      });
      this.metrics.placementTime.record(
        update.placementTimeMs ?? 0,
        sessionAttributes(state),
      );
      this.span.addEvent("tetris.piece.locked", attributes);
      emit(this.logger, "tetris.piece.locked", attributes);
    }
    if (update.linesCleared > 0) {
      const attributes = {
        ...sessionAttributes(state),
        "tetris.lines.cleared": update.linesCleared,
        "tetris.score": state.score,
      };
      this.metrics.linesCleared.add(update.linesCleared);
      this.span.addEvent("tetris.lines.cleared", attributes);
      emit(this.logger, "tetris.lines.cleared", attributes);
    }
    if (update.spawnedPiece !== undefined) {
      this.metrics.piecesSpawned.add(1, {
        "tetris.piece.kind": update.spawnedPiece,
      });
      emit(this.logger, "tetris.piece.spawned", {
        ...sessionAttributes(state),
        "tetris.piece.kind": update.spawnedPiece,
      });
    }
    if (update.gameOver && !this.before.gameOver) {
      const attributes = stateAttributes(state);
      this.metrics.gamesCompleted.add(1);
      this.span.addEvent("tetris.game.over", attributes);
      emit(this.logger, "tetris.game.over", attributes);
    }
  }
}

function createMetrics(meter: Meter): TetrisSessionMetrics {
  return {
    activeSessions: meter.createUpDownCounter("tetris.session.active", {
      description: "Number of active in-process Tetris sessions.",
      unit: "{session}",
    }),
    boardPressure: meter.createGauge("tetris.board.pressure", {
      description: "Current occupied board-height ratio by session.",
      unit: "1",
    }),
    commands: meter.createCounter("tetris.command", {
      description: "Programmatic Tetris commands by command and outcome.",
      unit: "{command}",
    }),
    dropDistance: meter.createHistogram("tetris.drop.distance", {
      description: "Vertical rows traversed by drop commands.",
      unit: "{row}",
    }),
    gamesCompleted: meter.createCounter("tetris.game.completed", {
      description: "Tetris games that reached game over.",
      unit: "{game}",
    }),
    gamesStarted: meter.createCounter("tetris.game.started", {
      description: "Tetris games started, including restarts.",
      unit: "{game}",
    }),
    level: meter.createGauge("tetris.level", {
      description: "Current Tetris level by session.",
      unit: "{level}",
    }),
    linesCleared: meter.createCounter("tetris.lines.cleared", {
      description: "Tetris lines cleared.",
      unit: "{line}",
    }),
    piecesLocked: meter.createCounter("tetris.piece.locked", {
      description: "Tetris pieces locked by piece kind.",
      unit: "{piece}",
    }),
    piecesSpawned: meter.createCounter("tetris.piece.spawned", {
      description: "Tetris pieces spawned by piece kind.",
      unit: "{piece}",
    }),
    placementTime: meter.createHistogram("tetris.placement_time", {
      description: "Piece placement duration for future evidence selection.",
      unit: "ms",
    }),
    recoveryFailure: meter.createCounter("tetris.recovery_failure", {
      description: "Rejected recovery actions for future evidence selection.",
      unit: "{failure}",
    }),
    score: meter.createGauge("tetris.score", {
      description: "Current Tetris score by session.",
      unit: "{point}",
    }),
  };
}

function commandOutcome(
  transition: TetrisSessionTransition,
): "applied" | "ignored" | "rejected" {
  if (transition.update?.recoveryFailureRecorded === true) return "rejected";
  return transition.changed ? "applied" : "ignored";
}

function sessionAttributes(state: TetrisSessionState): Attributes {
  return { "tetris.session.id": state.sessionId };
}

function stateAttributes(state: TetrisSessionState): Attributes {
  return {
    ...sessionAttributes(state),
    "tetris.board.pressure": state.dropObservation.boardPressure,
    "tetris.game.status": state.status,
    "tetris.level": state.level,
    "tetris.lines": state.lines,
    "tetris.piece.kind": state.activePiece,
    "tetris.score": state.score,
    "tetris.session.revision": state.revision,
  };
}

function emit(
  logger: Logger,
  eventName: string,
  attributes: Attributes,
  severityNumber = SeverityNumber.INFO,
): void {
  logger.emit({
    eventName,
    severityNumber,
    severityText: severityNumber >= SeverityNumber.ERROR
      ? "ERROR"
      : severityNumber >= SeverityNumber.WARN
        ? "WARN"
        : "INFO",
    attributes,
  });
}
