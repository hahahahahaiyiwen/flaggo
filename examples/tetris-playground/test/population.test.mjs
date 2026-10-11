import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { createFlaggoDropIntervalProvider } from "../../tetris-integration/dist/flaggo/flaggo-provider.js";
import { TetrisPopulation } from "../dist/population.js";

function harness(context, options = {}) {
  let clock = 0;
  const windows = [];
  const inputs = [];
  context.mock.timers.enable({ apis: ["setTimeout", "setInterval"] });
  const population = new TetrisPopulation({
    mode: "live",
    now: () => clock,
    onChanged() {},
    recordExperience(window) { windows.push(window); },
    createProvider(configuration) {
      return {
        async select(context) {
          configuration.experience.observe(configuration.state(), configuration.now());
          const input = configuration.experience.runtimeInput(context);
          inputs.push(input);
          const selection = { intervalMs: 800, source: "flaggo", status: "test", exposureId: input.exposure_id };
          configuration.experience.registerDecision(input, selection.intervalMs);
          configuration.onDecision?.({
            sessionId: context.sessionId, attributes: input, observedAt: "2026-10-08T00:00:00Z",
            result: selection.intervalMs, source: selection.source, status: selection.status, latencyMs: 0,
          });
          return selection;
        },
      };
    },
    ...options,
  });
  context.after(() => population.close());
  return {
    population, windows, inputs,
    async advance(to) {
      while (clock < to) {
        const next = Math.min(to, clock + 50);
        const delta = next - clock;
        clock = next;
        context.mock.timers.tick(delta);
        for (let turn = 0; turn < 30; turn += 1) await Promise.resolve();
      }
    },
  };
}

function authoredInterval(random, mode = "live") {
  return mode === "benchmark"
    ? random < 0.5 ? 200 : 1_000
    : [200, 400, 600, 800, 1_000][Math.floor(random * 5)];
}

function sdkHarness(context, options = {}, selectInterval = (attributes) =>
  attributes.active_play_ms_5s === 0 || attributes.pieces_locked_5s === 0
    ? attributes.current_drop_interval_ms
    : authoredInterval(attributes._random, options.mode)
) {
  const requests = [];
  const contractDigest = `sha256:${"0".repeat(64)}`;
  const h = harness(context, {
    ...options,
    createProvider(configuration) {
      return createFlaggoDropIntervalProvider({
        ...configuration,
        runtimeConfig: {
          format: "flaggo.runtime-config/v1",
          authority: { tenant: "local", application: "tetris", environment: "test" },
          services: {
            contractServiceUrl: "https://contracts.test",
            decisionServiceUrl: "https://decisions.test",
            otlpIngestionUrl: "https://telemetry.test",
          },
          bindings: { "tetris.dropInterval": { contractDigest } },
        },
        fetch: async (_input, init) => {
          const { attributes } = JSON.parse(String(init.body));
          requests.push(attributes);
          return new Response(JSON.stringify({
            contractDigest,
            executableDigest: `sha256:${"1".repeat(64)}`,
            result: selectInterval(attributes),
            evaluation: { source: "rule", rule: "test-policy" },
          }), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          });
        },
      });
    },
  });
  return { ...h, requests };
}

function assertSessionRandom(sessionId, requests) {
  const values = requests.filter((input) => input.session_id === sessionId)
    .map((input) => input._random);
  assert.ok(values.length > 0);
  const expected = Number.parseInt(
    createHash("sha256").update(sessionId).digest("hex").slice(0, 8), 16,
  ) / 2 ** 32;
  for (const value of values) {
    assert.ok(Number.isFinite(value) && value >= 0 && value < 1);
    assert.equal(value, expected);
  }
  return expected;
}

for (const mode of ["live", "benchmark"]) {
  test(`${mode} human SDK randomness is session-hashed across refreshes and restart`, async (context) => {
    const h = sdkHarness(context, { mode });
    h.population.start();
    await h.advance(1_000);
    const firstId = h.population.snapshot().human.state.sessionId;
    assert.equal(h.population.snapshot().human.state.dropInterval.intervalMs, 800);
    const firstInterval = authoredInterval(assertSessionRandom(firstId, h.requests), mode);
    h.population.command("hard-drop");
    for (const at of [2_000, 3_000]) {
      await h.advance(at);
      await h.population.refreshHumanPolicy();
      h.population.command("hard-drop");
      assert.equal(h.population.snapshot().human.state.dropInterval.intervalMs, firstInterval);
    }
    h.population.command("pause");
    h.population.command("resume");
    await h.advance(4_000);
    assert.ok(h.requests.filter((input) => input.session_id === firstId).length >= 3);
    assertSessionRandom(firstId, h.requests);

    h.population.command("restart");
    await h.advance(4_100);
    const restartedId = h.population.snapshot().human.state.sessionId;
    assert.notEqual(restartedId, firstId);
    const restartedInterval = authoredInterval(assertSessionRandom(restartedId, h.requests), mode);
    h.population.command("hard-drop");
    await h.advance(4_200);
    await h.population.refreshHumanPolicy();
    h.population.command("hard-drop");
    assert.equal(h.population.snapshot().human.state.dropInterval.intervalMs, restartedInterval);
    assertSessionRandom(restartedId, h.requests);
    assert.ok(h.requests.every((input) => Object.keys(input).length === 13));
  });
}

test("bot SDK randomness follows each episode's session ID through turnover and Stop/Start", async (context) => {
  const h = sdkHarness(context, { episodeDurationMs: 7_000 });
  h.population.startAll();
  const firstIds = h.population.snapshot().bots.map((slot) => slot.session.state.sessionId);
  await h.advance(7_200);
  const slots = h.population.snapshot().bots;
  for (const [index, slot] of slots.entries()) {
    assert.equal(slot.status, "running");
    assert.equal(slot.episode, 2);
    assert.notEqual(slot.session.state.sessionId, firstIds[index]);
    assert.ok(h.requests.filter((input) => input.session_id === firstIds[index]).length >= 2);
    assertSessionRandom(firstIds[index], h.requests);
    assertSessionRandom(slot.session.state.sessionId, h.requests);
  }
  await h.population.stopAll();
  h.population.startBot("bot-1");
  await h.advance(7_300);
  const restarted = h.population.snapshot().bots[0];
  assert.notEqual(restarted.session.state.sessionId, slots[0].session.state.sessionId);
  assertSessionRandom(restarted.session.state.sessionId, h.requests);
});

test("session-stable SDK randomness does not pin learned policy results", async (context) => {
  let learnedInterval = 600;
  const h = sdkHarness(context, {}, (attributes) =>
    attributes.active_play_ms_5s === 0 || attributes.pieces_locked_5s === 0
      ? attributes.current_drop_interval_ms : learnedInterval
  );
  h.population.start();
  await h.advance(1_000);
  h.population.command("hard-drop");
  await h.advance(2_000);
  await h.population.refreshHumanPolicy();
  h.population.command("hard-drop");
  assert.equal(h.population.snapshot().human.state.dropInterval.intervalMs, 600);
  learnedInterval = 1_200;
  await h.advance(3_000);
  await h.population.refreshHumanPolicy();
  h.population.command("hard-drop");
  assert.equal(h.population.snapshot().human.state.dropInterval.intervalMs, 1_200);
  assertSessionRandom(h.population.snapshot().human.state.sessionId, h.requests);
});

test("eleven independent sessions produce correctly attributed wall-clock human/bot evidence", async (context) => {
  const h = harness(context);
  assert.deepEqual(h.population.snapshot().bots.map((slot) => slot.id),
    Array.from({ length: 10 }, (_, index) => `bot-${index + 1}`));
  assert.ok(h.population.snapshot().bots.every((slot) => slot.status === "idle"));
  h.population.start();
  h.population.command("hard-drop");
  h.population.startAll();
  await h.advance(7_000);
  const snapshot = h.population.snapshot();
  assert.equal(new Set([snapshot.human.state.sessionId,
    ...snapshot.bots.map((slot) => slot.session.state.sessionId)]).size, 11);
  assert.equal(snapshot.human.decision.sessionId, snapshot.human.state.sessionId);
  assert.ok(snapshot.bots.every((slot) => slot.session.decision.sessionId === slot.session.state.sessionId));
  assert.ok(h.windows.some((window) => window.origin === "human"));
  assert.ok(h.windows.some((window) => window.origin === "greedy"));
  for (const slot of snapshot.bots) {
    assert.ok(h.windows.some((window) => window.input.session_id === slot.session.state.sessionId));
  }
  assert.ok(h.windows.every((window) => window.dataset === "live" && window.clock === "wall"));
  assert.equal(new Set(h.inputs.map((input) => input.exposure_id)).size, h.inputs.length);
  const oldId = snapshot.human.state.sessionId;
  h.population.command("restart");
  const restarted = h.population.snapshot();
  assert.notEqual(restarted.human.state.sessionId, oldId);
  assert.ok(restarted.human.totals.activePlayMs >= snapshot.human.experience.activePlayMs);
  assert.deepEqual(restarted.bots.map((slot) => slot.session.state.sessionId),
    snapshot.bots.map((slot) => slot.session.state.sessionId));
});

test("human input does not delay gravity and pausing the human does not pause bots", async (context) => {
  const operations = [];
  const h = harness(context, {
    instrumentation: {
      sessionStarted() {}, sessionClosed() {},
      commandStarted(operation, state) {
        operations.push({ operation, id: state.sessionId });
        return { completed() {}, failed() {} };
      },
    },
  });
  h.population.start();
  for (let at = 100; at <= 900; at += 100) {
    await h.advance(at);
    h.population.command("move-left");
  }
  assert.ok(operations.some((value) => value.operation === "gravity-tick"));
  h.population.command("pause");
  const human = h.population.snapshot().human.state;
  h.population.startAll();
  await h.advance(5_000);
  assert.deepEqual(h.population.snapshot().human.state, human);
  assert.ok(h.population.snapshot().bots.every((slot) => slot.totals.activePlayMs > 0));
  await h.population.stopAll();
  assert.ok(h.population.snapshot().bots.every((slot) => slot.status === "idle"));
});

test("continuous slots create fresh episodes, while Stop preserves outcomes without a completion", async (context) => {
  const h = harness(context, { episodeDurationMs: 1_000 });
  h.population.startBot("bot-1");
  const firstId = h.population.snapshot().bots[0].session.state.sessionId;
  await h.advance(1_200);
  const slot = h.population.snapshot().bots[0];
  assert.equal(slot.episode, 2);
  assert.notEqual(slot.session.state.sessionId, firstId);
  assert.equal(h.windows.filter((window) => window.episodeComplete).length, 1);
  await h.population.stopBot("bot-1");
  await h.advance(2_000);
  const stopped = h.population.snapshot().bots[0];
  assert.equal(stopped.status, "idle");
  assert.equal(stopped.episode, 2);
  assert.equal(stopped.totals.activePlayMs, 1_200);
  assert.equal(h.windows.filter((window) => window.episodeComplete).length, 1);
});

test("unexpected reporting errors remain failed rather than automatically restarting", async (context) => {
  const h = harness(context, { recordExperience() { throw new Error("reporter failed"); } });
  h.population.startBot("bot-1");
  await h.advance(7_000);
  const slot = h.population.snapshot().bots[0];
  assert.equal(slot.status, "failed");
  assert.equal(slot.episode, 1);
  assert.match(slot.error, /reporter failed/u);
  h.population.startAll();
  assert.equal(h.population.snapshot().bots[0].episode, 1);
});

test("Stop does not hide reporting failure while closing the episode", async (context) => {
  const h = harness(context, { recordExperience() { throw new Error("close reporter failed"); } });
  h.population.startBot("bot-1");
  await h.advance(1_000);
  await h.population.stopBot("bot-1");
  const slot = h.population.snapshot().bots[0];
  assert.equal(slot.status, "failed");
  assert.match(slot.error, /close reporter failed/u);
  assert.equal(slot.totals.activePlayMs, 1_000);
});

test("stopped and replaced sessions ignore late provider decisions", async (context) => {
  const pending = [];
  const h = harness(context, {
    createProvider(configuration) {
      return {
        select(context) {
          return new Promise((resolve) => pending.push({ configuration, context, resolve }));
        },
      };
    },
  });
  h.population.start();
  h.population.startBot("bot-1");
  await h.advance(100);
  const old = h.population.snapshot();
  const retired = pending.slice();
  h.population.command("restart");
  await h.population.stopBot("bot-1");
  h.population.startBot("bot-1");
  await h.advance(200);
  for (const { configuration, context, resolve } of retired) {
    configuration.onDecision({
      sessionId: context.sessionId, attributes: {}, observedAt: "2026-10-08T00:00:00Z",
      result: 200, source: "flaggo", status: "late", latencyMs: 100,
    });
    resolve({ intervalMs: 200, source: "flaggo", status: "late" });
  }
  await h.advance(250);
  const current = h.population.snapshot();
  assert.notEqual(current.human.state.sessionId, old.human.state.sessionId);
  assert.notEqual(current.bots[0].session.state.sessionId, old.bots[0].session.state.sessionId);
  assert.equal(current.human.decision, undefined);
  assert.equal(current.bots[0].session.decision, undefined);
  assert.equal(current.bots[0].session.state.dropInterval.intervalMs, 800);
  assert.equal(current.bots[0].status, "running");
});

test("a late human response does not reset gravity for an already applied interval", async (context) => {
  const operations = [];
  const replies = [];
  let clock = 0;
  const h = harness(context, {
    instrumentation: {
      sessionStarted() {}, sessionClosed() {},
      commandStarted(operation) {
        operations.push({ operation, at: clock });
        return { completed() {}, failed() {} };
      },
    },
    now: () => clock,
    createProvider() {
      return { select() { return new Promise((resolve) => replies.push(resolve)); } };
    },
  });
  const advance = async (to) => {
    while (clock < to) {
      clock = Math.min(to, clock + 50);
      await h.advance(clock);
    }
  };
  h.population.start();
  await advance(1_000);
  replies[0]({ intervalMs: 200, source: "flaggo", status: "test" });
  await advance(5_000);
  assert.equal(replies.length, 2);
  await advance(5_200);
  h.population.command("hard-drop");
  await advance(5_300);
  replies[1]({ intervalMs: 200, source: "flaggo", status: "test" });
  await advance(5_400);
  assert.ok(operations.some((value) => value.operation === "gravity-tick" && value.at === 5_400));
});

test("benchmark preview never exports live learning windows or spawns bots", async (context) => {
  const h = harness(context, { mode: "benchmark" });
  h.population.start();
  h.population.command("hard-drop");
  await h.advance(7_000);
  await h.population.close();
  assert.equal(h.windows.length, 0);
  assert.deepEqual(h.population.snapshot().bots, []);
  assert.throws(() => h.population.startBot("bot-1"), /closed/u);
});
