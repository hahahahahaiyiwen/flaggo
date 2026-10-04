import assert from "node:assert/strict";
import test from "node:test";

import { context as otelContext } from "@opentelemetry/api";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  InMemoryLogRecordExporter,
  LoggerProvider,
  SimpleLogRecordProcessor,
} from "@opentelemetry/sdk-logs";
import {
  AggregationTemporality,
  InMemoryMetricExporter,
  MeterProvider,
  PeriodicExportingMetricReader,
} from "@opentelemetry/sdk-metrics";
import {
  InMemorySpanExporter,
  SimpleSpanProcessor,
  TracerProvider,
} from "@opentelemetry/sdk-trace";

import { SequencePieceSource } from "../dist/flaggo/game.js";
import {
  createTetrisSessionInstrumentation,
  tetrisEngineInstrumentationScope,
} from "../dist/flaggo/session-telemetry.js";
import { TetrisSession } from "../dist/flaggo/session.js";

test("headless sessions emit correlated engine telemetry with bounded attributes", async () => {
  const resource = resourceFromAttributes({ "service.name": "tetris-test" });
  const logExporter = new InMemoryLogRecordExporter();
  const metricExporter = new InMemoryMetricExporter(
    AggregationTemporality.CUMULATIVE,
  );
  const spanExporter = new InMemorySpanExporter();
  const metricReader = new PeriodicExportingMetricReader({
    exporter: metricExporter,
    exportIntervalMillis: 60_000,
  });
  const loggerProvider = new LoggerProvider({
    resource,
    processors: [
      new SimpleLogRecordProcessor({ exporter: logExporter }),
    ],
  });
  const meterProvider = new MeterProvider({
    resource,
    readers: [metricReader],
  });
  const tracerProvider = new TracerProvider({
    resource,
    spanProcessors: [
      new SimpleSpanProcessor({ exporter: spanExporter }),
    ],
  });
  const contextManager = new AsyncLocalStorageContextManager().enable();
  assert.equal(otelContext.setGlobalContextManager(contextManager), true);

  try {
    const instrumentation = createTetrisSessionInstrumentation({
      logger: loggerProvider.getLogger(tetrisEngineInstrumentationScope),
      meter: meterProvider.getMeter(tetrisEngineInstrumentationScope),
      tracer: tracerProvider.getTracer(tetrisEngineInstrumentationScope),
    });
    const firstClock = deterministicClock();
    const first = new TetrisSession({
      game: {
        pieceSource: new SequencePieceSource(["O"]),
        sessionId: "telemetry-session-a",
      },
      instrumentation,
      now: firstClock.now,
    });
    first.dispatch("pause");
    first.dispatch("resume");
    for (const horizontalOffset of [-4, -2, 0, 2, 4]) {
      moveHorizontally(first, horizontalOffset);
      firstClock.advance(100);
      first.dispatch("hard-drop");
    }
    assert.equal(first.snapshot().lines, 2);
    first.dispatch("restart");

    const secondClock = deterministicClock();
    const second = new TetrisSession({
      game: {
        pieceSource: new SequencePieceSource(["O"]),
        sessionId: "telemetry-session-b",
      },
      instrumentation,
      now: secondClock.now,
    });
    moveHorizontally(second, -4);
    second.dispatch("move-left");
    second.dispatch("move-left");
    second.dispatch("move-left");
    secondClock.advance(100);
    second.dispatch("hard-drop");
    for (let count = 0; !second.snapshot().gameOver; count += 1) {
      assert.ok(count < 20, "deterministic game should reach game over");
      secondClock.advance(100);
      second.dispatch("hard-drop");
    }

    first.close();
    second.close();
    await Promise.all([
      loggerProvider.forceFlush(),
      meterProvider.forceFlush(),
      tracerProvider.forceFlush(),
    ]);

    const logs = logExporter.getFinishedLogRecords();
    const eventNames = new Set(logs.map((record) => record.eventName));
    assert.deepEqual(eventNames, new Set([
      "tetris.game.over",
      "tetris.game.paused",
      "tetris.game.restarted",
      "tetris.game.resumed",
      "tetris.game.started",
      "tetris.lines.cleared",
      "tetris.piece.locked",
      "tetris.piece.spawned",
      "tetris.recovery.failed",
    ]));
    assert.ok(logs.every((record) =>
      record.instrumentationScope.name === tetrisEngineInstrumentationScope
    ));
    assert.ok(logs.every((record) =>
      !Object.hasOwn(record.attributes, "tetris.board")
    ));

    const spans = spanExporter.getFinishedSpans();
    assert.ok(spans.length > 0);
    assert.ok(spans.every((span) =>
      span.name === "tetris.command"
      && span.instrumentationScope.name === tetrisEngineInstrumentationScope
    ));
    const commandTraceIds = new Set(
      spans.map((span) => span.spanContext().traceId),
    );
    assert.ok(
      logs
        .filter((record) =>
          record.eventName === "tetris.piece.locked"
          || record.eventName === "tetris.recovery.failed"
        )
        .every((record) =>
          record.spanContext !== undefined
          && commandTraceIds.has(record.spanContext.traceId)
        ),
    );
    const spanEventNames = new Set(
      spans.flatMap((span) => span.events.map((event) => event.name)),
    );
    assert.ok(spanEventNames.has("tetris.piece.locked"));
    assert.ok(spanEventNames.has("tetris.lines.cleared"));
    assert.ok(spanEventNames.has("tetris.game.over"));

    const metrics = metricExporter.getMetrics().flatMap((resourceMetrics) =>
      resourceMetrics.scopeMetrics.flatMap((scope) => scope.metrics)
    );
    assert.deepEqual(
      new Set(metrics.map((metric) => metric.descriptor.name)),
      new Set([
        "tetris.board.pressure",
        "tetris.command",
        "tetris.drop.distance",
        "tetris.game.completed",
        "tetris.game.started",
        "tetris.level",
        "tetris.lines.cleared",
        "tetris.piece.locked",
        "tetris.piece.spawned",
        "tetris.placement_time",
        "tetris.recovery_failure",
        "tetris.score",
        "tetris.session.active",
      ]),
    );
    assert.equal(metric(metrics, "tetris.placement_time").descriptor.unit, "ms");
    assert.equal(
      metric(metrics, "tetris.recovery_failure").descriptor.unit,
      "{failure}",
    );
    assert.deepEqual(
      new Set(
        metric(metrics, "tetris.placement_time").dataPoints.map(
          (point) => point.attributes["tetris.session.id"],
        ),
      ),
      new Set(["telemetry-session-a", "telemetry-session-b"]),
    );
    assert.ok(
      metric(metrics, "tetris.recovery_failure").dataPoints.some((point) =>
        point.attributes["tetris.session.id"] === "telemetry-session-b"
      ),
    );
    assert.ok(
      metric(metrics, "tetris.score").dataPoints.every((point) =>
        typeof point.attributes["tetris.session.id"] === "string"
      ),
    );
    assert.ok(
      metric(metrics, "tetris.command").dataPoints.every((point) =>
        point.attributes["tetris.session.id"] === undefined
        && typeof point.attributes["tetris.command"] === "string"
        && typeof point.attributes["tetris.command.outcome"] === "string"
      ),
    );
  } finally {
    await Promise.all([
      loggerProvider.shutdown(),
      meterProvider.shutdown(),
      tracerProvider.shutdown(),
    ]);
    otelContext.disable();
    contextManager.disable();
  }
});

function deterministicClock() {
  let current = 0;
  return {
    now: () => current,
    advance(milliseconds) {
      current += milliseconds;
    },
  };
}

function moveHorizontally(session, offset) {
  const command = offset < 0 ? "move-left" : "move-right";
  for (let count = 0; count < Math.abs(offset); count += 1) {
    assert.equal(session.dispatch(command).changed, true);
  }
}

function metric(metrics, name) {
  const found = metrics.find((candidate) => candidate.descriptor.name === name);
  assert.ok(found, `missing metric '${name}'`);
  return found;
}
