import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:http";
import test from "node:test";

import { createExperienceRecorder } from "../dist/flaggo/experience.js";
import { TetrisTelemetryProviders } from "../dist/flaggo/otel.js";

test("Tetris exports new windows only, preserving all four gauges, retries, and shutdown flushing",
  { timeout: 20_000 }, async (context) => {
    const accepted = [];
    const attempts = [];
    let retryNext = false;
    const server = createServer(async (request, response) => {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const payload = JSON.parse(Buffer.concat(chunks).toString());
      if (request.url === "/v1/metrics") {
        attempts.push(payload);
        if (retryNext) {
          retryNext = false;
          response.writeHead(503, { "Retry-After": "1" }).end();
          return;
        }
        accepted.push(payload);
      }
      response.writeHead(200, { "Content-Type": "application/json" }).end("{}");
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const url = `http://127.0.0.1:${server.address().port}`;
    context.after(() => new Promise((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve())));
    const telemetry = new TetrisTelemetryProviders({
      capture: true,
      runtimeConfig: {
        format: "flaggo.runtime-config/v1",
        authority: { tenant: "local", application: "tetris", environment: "otel-test" },
        services: { contractServiceUrl: url, decisionServiceUrl: url, otlpIngestionUrl: url },
        bindings: {
          "tetris.dropInterval": { contractDigest: `sha256:${"a".repeat(64)}` },
        },
      },
    });
    let closed = false;
    context.after(async () => { if (!closed) await telemetry.shutdown(); });
    const record = createExperienceRecorder(telemetry.getMeter("tetris.experience"));
    const expected = new Map();
    const addWindow = (id, slot, terminal = false) => {
      const window = {
        windowId: id,
        input: {
          session_id: `bot-${slot}`, exposure_id: `exposure-${id}`,
          board_pressure_mean_5s: 0.1, hard_drop_ratio_5s: 1, pieces_locked_5s: 2,
          active_play_ms_5s: 5_000,
        },
        dataset: "live", origin: "greedy", clock: "wall",
        intervalMs: [200, 400, 600, 800, 1_000][slot % 5],
        startAtMs: 0, endAtMs: 5_000, activePlayMs: terminal ? 0 : 5_000,
        challengeBalance: terminal ? 0 : 0.8, linesCleared: terminal ? 0 : 2,
        gameOvers: terminal ? 1 : 0, episodeComplete: terminal,
      };
      expected.set(id, window);
      record(window);
    };

    // Exceed the default lifetime cardinality limit across collections.
    for (let cycle = 0; cycle < 25; cycle += 1) {
      for (let slot = 0; slot < 10; slot += 1) {
        for (let index = 0; index < 10; index += 1) {
          addWindow(`${cycle}-${slot}-${index}`, slot);
        }
      }
      const previousCount = accepted.length;
      await telemetry.forceFlush();
      assert.equal(accepted.length, previousCount + 1);
      const points = otlpPoints(accepted.at(-1));
      assert.equal(points.length, 400);
      assert.ok(points.every((point) =>
        attribute(point, "tetris.experience.window.id").startsWith(`${cycle}-`)));
    }
    const previousCount = accepted.length;
    for (let collection = 0; collection < 3; collection += 1) await telemetry.forceFlush();
    assert.equal(accepted.length, previousCount, "idle collection must not re-export old windows");

    retryNext = true;
    addWindow("retry", 0);
    const previousAttempts = attempts.length;
    await telemetry.forceFlush();
    assert.equal(attempts.length, previousAttempts + 2);
    assert.deepEqual(attempts.at(-2), attempts.at(-1), "OTLP retry preserves the collected payload");
    assert.equal(otlpPoints(accepted.at(-1)).length, 4);

    addWindow("periodic", 1);
    const deadline = Date.now() + 7_000;
    while (!otlpPoints(accepted.at(-1)).some((point) =>
      attribute(point, "tetris.experience.window.id") === "periodic")) {
      assert.ok(Date.now() < deadline, "five-second periodic export did not deliver the new window");
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    assert.equal(otlpPoints(accepted.at(-1)).length, 4);

    addWindow("terminal", 2, true);
    await telemetry.shutdown();
    closed = true;
    assert.equal(otlpPoints(accepted.at(-1)).length, 4, "shutdown flushes the final window only");
    const points = accepted.flatMap(otlpPoints);
    assert.equal(points.length, expected.size * 4);
    const seen = new Set();
    for (const point of points) {
      const id = attribute(point, "tetris.experience.window.id");
      const window = expected.get(id);
      assert.ok(window);
      const key = `${id}/${point.name}`;
      assert.ok(!seen.has(key), `repeated point ${key}`);
      seen.add(key);
      assert.equal(attribute(point, "tetris.session.id"), window.input.session_id);
      assert.equal(attribute(point, "tetris.exposure.id"), window.input.exposure_id);
      assert.equal(Number(attribute(point, "tetris.drop_interval.ms")), window.intervalMs);
      const values = {
        "tetris.experience.challenge_balance": window.challengeBalance,
        "tetris.experience.lines_cleared": window.linesCleared,
        "tetris.experience.game_over_count": window.gameOvers,
        "tetris.experience.active_play_ms": window.activePlayMs,
      };
      assert.ok(Object.hasOwn(values, point.name));
      assert.equal(Number(point.asDouble ?? point.asInt), values[point.name]);
    }
    const captured = telemetry.metrics.flatMap((resource) =>
      resource.scopeMetrics.flatMap((scope) =>
        scope.metrics.filter((metric) => metric.descriptor.name.startsWith("tetris.experience."))
          .flatMap((metric) => metric.dataPoints)));
    assert.equal(captured.length, expected.size * 4, "capture reader also avoids repeated windows");
  });

function otlpPoints(payload) {
  return payload.resourceMetrics.flatMap((resource) =>
    resource.scopeMetrics.flatMap((scope) =>
      scope.metrics.flatMap((metric) =>
        (metric.gauge?.dataPoints ?? []).map((point) => ({ ...point, name: metric.name })))));
}

function attribute(point, key) {
  const value = point.attributes.find((entry) => entry.key === key)?.value;
  return value?.stringValue ?? value?.intValue ?? value?.doubleValue ?? value?.boolValue;
}
