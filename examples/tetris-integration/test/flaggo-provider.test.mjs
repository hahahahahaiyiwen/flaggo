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

import {
  createFlaggoDropIntervalProvider,
} from "../dist/flaggo/flaggo-provider.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";

const context = {
  boardPressureMean5s: 0.8,
  boardPressureMax5s: 0.9,
  currentLevel: 8,
  placementTimeMeanMs5s: 1_600,
  recoveryFailures5s: 3,
  piecesLocked5s: 2,
  sessionId: "provider-test",
};

function decisionResponse(result, evaluation = { source: "rule", rule: "high-pressure" }) {
  return new Response(JSON.stringify({
    contractDigest,
    executableDigest,
    result,
    evaluation,
  }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

test("Flaggo provider maps game context and returns decision provenance", async () => {
  let request;
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async (input, init) => {
      request = { input, init };
      return decisionResponse(850);
    },
  });

  assert.deepEqual(await provider.select(context), {
    intervalMs: 850,
    source: "flaggo",
    status: "rule high-pressure",
  });
  assert.match(
    String(request.input),
    /\/v3\/decision-contracts\/tetris\.dropInterval\/versions\//u,
  );
  const body = JSON.parse(String(request.init.body));
  assert.deepEqual(
    {
      ...body.attributes,
      _random: undefined,
    },
    {
      board_pressure_mean_5s: 0.8,
      board_pressure_max_5s: 0.9,
      current_level: 8,
      placement_time_mean_ms_5s: 1_600,
      recovery_failures_5s: 3,
      pieces_locked_5s: 2,
      session_id: "provider-test",
      _random: undefined,
    },
  );
  assert.equal(typeof body.attributes._random, "number");
});

test("Flaggo provider emits application metrics, logs, and a correlated decision span", async () => {
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
    const provider = createFlaggoDropIntervalProvider({
      baseUrl: "https://decisions.test",
      contractDigest,
      retry: { maxAttempts: 1 },
      fetch: async () => decisionResponse(850),
      telemetry: {
        flaggoLogger: loggerProvider.getLogger("@flaggo/sdk", "0.2.0"),
        logger: loggerProvider.getLogger("tetris.policy"),
        meter: meterProvider.getMeter("tetris.policy"),
        tracer: tracerProvider.getTracer("tetris.policy"),
      },
    });

    assert.equal((await provider.select(context)).intervalMs, 850);
    await Promise.all([
      loggerProvider.forceFlush(),
      meterProvider.forceFlush(),
      tracerProvider.forceFlush(),
    ]);

    const records = logExporter.getFinishedLogRecords();
    assert.deepEqual(
      records.map((record) => record.eventName).sort(),
      ["flaggo.decision.received", "tetris.drop_interval.selected"],
    );
    assert.equal(
      records.find((record) =>
        record.eventName === "flaggo.decision.received"
      )?.instrumentationScope.name,
      "@flaggo/sdk",
    );
    assert.equal(
      records.find((record) =>
        record.eventName === "tetris.drop_interval.selected"
      )?.instrumentationScope.name,
      "tetris.policy",
    );
    const span = spanExporter.getFinishedSpans()[0];
    assert.ok(span);
    assert.equal(span.name, "tetris.drop_interval.select");
    assert.equal(span.attributes["tetris.drop_interval.ms"], 850);
    assert.equal(
      records[0].spanContext?.traceId,
      span.spanContext().traceId,
    );
    assert.equal(
      records[1].spanContext?.traceId,
      span.spanContext().traceId,
    );

    const metricNames = metricExporter.getMetrics().flatMap((resourceMetrics) =>
      resourceMetrics.scopeMetrics.flatMap((scope) =>
        scope.metrics.map((metric) => metric.descriptor.name)
      )
    );
    assert.deepEqual(metricNames.sort(), [
      "tetris.board_pressure_max_5s",
      "tetris.board_pressure_mean_5s",
      "tetris.current_level",
      "tetris.pieces_locked_5s",
      "tetris.placement_time_mean_ms_5s",
      "tetris.recovery_failures_5s",
    ]);
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

test("Flaggo provider visibly falls back to the local application policy", async () => {
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async () => {
      throw new TypeError("connection refused");
    },
  });

  const selection = await provider.select(context);
  assert.equal(selection.intervalMs, 400);
  assert.equal(selection.source, "local-fallback");
  assert.match(selection.status, /^Flaggo unavailable: /u);
  assert.match(selection.status, /could not reach the service/u);
});

test("Flaggo provider rejects unusable results through local fallback", async () => {
  const provider = createFlaggoDropIntervalProvider({
    baseUrl: "https://decisions.test",
    contractDigest,
    retry: { maxAttempts: 1 },
    fetch: async () => decisionResponse(825, { source: "default" }),
  });

  const selection = await provider.select(context);
  assert.equal(selection.intervalMs, 400);
  assert.equal(selection.source, "local-fallback");
  assert.match(selection.status, /outside the contract bounds/u);
});
