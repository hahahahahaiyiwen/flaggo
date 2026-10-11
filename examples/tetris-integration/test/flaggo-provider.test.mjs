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
import { createExperienceRecorder, TetrisExperience } from "../dist/flaggo/experience.js";
import { SequencePieceSource } from "../dist/flaggo/game.js";
import { TetrisSession } from "../dist/flaggo/session.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const runtimeConfig = {
  format: "flaggo.runtime-config/v1",
  authority: {
    tenant: "local",
    application: "tetris",
    environment: "test",
  },
  services: {
    contractServiceUrl: "https://contracts.test",
    decisionServiceUrl: "https://decisions.test",
    otlpIngestionUrl: "https://telemetry.test",
  },
  bindings: {
    "tetris.dropInterval": { contractDigest },
  },
};

const context = {
  boardPressureMean5s: 0.8,
  boardPressureMax5s: 0.9,
  currentLevel: 8,
  placementTimeMeanMs5s: 1_600,
  recoveryFailures5s: 3,
  piecesLocked5s: 2,
  sessionId: "provider-test",
};

function decisionResponse(result, evaluation = { source: "rule", rule: "explore-fast-gravity" }) {
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

function harness(configuration, onWindow) {
  let clock = 0;
  let session;
  const experience = new TetrisExperience(
    { dataset: "training", origin: "greedy", clock: "virtual" }, onWindow);
  const provider = createFlaggoDropIntervalProvider({
    runtimeConfig,
    retry: { maxAttempts: 1 },
    ...configuration,
    experience,
    state: () => session.snapshot(),
    now: () => clock,
  });
  session = new TetrisSession({
    provider,
    game: { pieceSource: new SequencePieceSource(["O"]), sessionId: context.sessionId },
    fallbackIntervalMs: 1_000,
    policyApplication: "piece-boundary",
    now: () => clock,
  });
  experience.observe(session.snapshot(), clock);
  return {
    provider,
    session,
    experience,
    at(value) { clock = value; },
    command(command) {
      const transition = session.dispatch(command);
      experience.observe(transition.state, clock, transition);
      return transition;
    },
    async refresh() {
      const transition = await session.refreshPolicy();
      experience.observe(transition.state, clock, transition);
      return transition;
    },
    close() {
      session.close();
      experience.observe(session.snapshot(), clock);
    },
  };
}

test("Flaggo provider maps twelve adaptive attributes and reserves exposure correlation", async () => {
  let request;
  const h = harness({
    random: () => 0.25,
    fetch: async (input, init) => {
      request = { input, init };
      return decisionResponse(200);
    },
  });
  h.at(1_000);
  h.command("hard-drop");
  h.at(2_000);
  h.command("hard-drop");
  h.at(3_500);
  const selection = await h.provider.select(context);
  assert.match(selection.exposureId, /^exposure-/u);
  assert.deepEqual({ ...selection, exposureId: undefined }, {
    intervalMs: 200,
    source: "flaggo",
    status: "rule explore-fast-gravity",
    exposureId: undefined,
    contractDigest,
    executableDigest,
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
      hard_drop_ratio_5s: 1,
      placement_time_mean_ms_5s: 1_600,
      pieces_locked_5s: 2,
      active_play_ms_5s: 3_500,
      challenge_balance_5s: 1_500 / 3_500,
      lines_cleared_5s: 0,
      game_over_count_5s: 0,
      current_drop_interval_ms: 1_000,
      session_id: "provider-test",
      exposure_id: selection.exposureId,
      _random: undefined,
    },
  );
  assert.equal(Object.keys(body.attributes).length, 13);
  assert.equal(body.attributes._random, 0.25);
  h.close();
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
    const h = harness({
      fetch: async () => decisionResponse(200),
      telemetry: {
        flaggoLogger: loggerProvider.getLogger("@flaggo/sdk", "0.2.0"),
        logger: loggerProvider.getLogger("tetris.policy"),
        tracer: tracerProvider.getTracer("tetris.policy"),
      },
    }, createExperienceRecorder(meterProvider.getMeter("tetris.experience")));

    assert.equal((await h.refresh()).state.dropInterval.intervalMs, 200);
    h.at(1_000);
    h.command("hard-drop");
    h.at(3_000);
    h.command("pause");
    h.close();
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
    const decisionRecord = records.find((record) =>
      record.eventName === "flaggo.decision.received"
    );
    const applicationRecord = records.find((record) =>
      record.eventName === "tetris.drop_interval.selected"
    );
    assert.equal(
      decisionRecord?.attributes["flaggo.contract.digest"],
      contractDigest,
    );
    assert.equal(
      Object.hasOwn(
        applicationRecord?.attributes ?? {},
        "flaggo.contract.digest",
      ),
      false,
    );
    const span = spanExporter.getFinishedSpans()[0];
    assert.ok(span);
    assert.equal(span.name, "tetris.drop_interval.select");
    assert.equal(span.attributes["tetris.drop_interval.ms"], 200);
    assert.equal(
      decisionRecord?.attributes["flaggo.correlation.exposure_id"],
      applicationRecord?.attributes["tetris.exposure.id"],
    );
    assert.equal(
      Object.hasOwn(span.attributes, "flaggo.contract.digest"),
      false,
    );
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
      "tetris.experience.active_play_ms",
      "tetris.experience.challenge_balance",
      "tetris.experience.game_over_count",
      "tetris.experience.lines_cleared",
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

test("Flaggo failure retains actual applied gravity instead of using level", async () => {
  const h = harness({
    fetch: async () => {
      throw new TypeError("connection refused");
    },
  });

  const selection = await h.provider.select(context);
  assert.equal(selection.intervalMs, 1_000);
  assert.equal(selection.source, "local-fallback");
  assert.equal(selection.exposureId, undefined);
  assert.match(selection.status, /^Flaggo unavailable: /u);
  assert.match(selection.status, /could not reach the service/u);
  h.close();
});

test("Flaggo provider rejects unusable results without registering an exposure", async () => {
  const h = harness({
    fetch: async () => decisionResponse(825, { source: "default" }),
  });

  const selection = await h.provider.select(context);
  assert.equal(selection.intervalMs, 1_000);
  assert.equal(selection.source, "local-fallback");
  assert.equal(selection.exposureId, undefined);
  assert.match(selection.status, /outside the contract bounds/u);
  h.close();
});

test("cached failures preserve the applied exposure and its outcome window", async () => {
  let requests = 0;
  const windows = [];
  const h = harness({
    fetch: async () => {
      if (++requests === 1) return decisionResponse(200);
      throw new TypeError("connection refused");
    },
  }, (window) => windows.push(window));
  const applied = (await h.refresh()).state.dropInterval;
  h.at(1_000);
  h.command("hard-drop");
  const cached = (await h.refresh()).state.dropInterval;
  assert.equal(cached.source, "flaggo-cached");
  assert.equal(cached.intervalMs, 200);
  assert.equal(cached.exposureId, applied.exposureId);
  assert.equal(cached.contractDigest, contractDigest);
  assert.equal(cached.executableDigest, executableDigest);
  h.at(2_000);
  h.close();
  assert.ok(windows.every((window) => window.input.exposure_id === applied.exposureId));
  assert.equal(windows.reduce((sum, window) => sum + window.activePlayMs, 0), 2_000);
});

test("aborted requests do not become successful fallback decisions", async () => {
  const controller = new AbortController();
  controller.abort();
  const h = harness({ fetch: async () => assert.fail("No transport after cancellation.") });
  await assert.rejects(() => h.provider.select(context, controller.signal));
  h.close();
});
