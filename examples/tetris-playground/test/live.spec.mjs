import { rm, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { startPlaygroundFixture } from "./playground-fixture.mjs";

let host;
let runDirectory;

test.beforeAll(async () => {
  test.setTimeout(660_000);
  host = await startPlaygroundFixture({ mode: "live", analysisMode: "copilot", retain: true });
  runDirectory = host.runDirectory;
});

test.afterAll(async () => {
  if (host === undefined) return;
  await host.close();
  await rm(runDirectory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
});

test("live human and ten real-time players feed observed Copilot evidence analysis", async ({ page }, testInfo) => {
  test.setTimeout(660_000);
  await page.goto(host.url);
  await expect(page.locator("#connection")).toHaveText("Live");
  await expect(page.locator(".bot-card")).toHaveCount(3);
  await expect(page.locator("#bots-range")).toHaveText("Bots 1-3 of 10");
  await expect(page.locator("#benchmark-panel")).toBeHidden();
  await page.locator("#bots-start").click();
  await expect(page.locator("#bot-count")).toHaveText("10 running");
  const collectionEndsAt = Date.now() + 60_000;
  const deadline = Date.now() + 600_000;
  let stopped = false;
  let stoppedHumanState;
  let evidence;
  let analysis = { sqlActions: [] };
  let current;
  let commandSequence = 0;
  while (Date.now() < deadline) {
    const response = await page.request.get(`${host.url}/api/state`);
    expect(response.ok()).toBe(true);
    current = await response.json();
    if (!stopped && Date.now() >= collectionEndsAt) {
      expect(current.bots.every((slot) => slot.status === "running")).toBe(true);
      const pause = await page.request.post(`${host.url}/api/commands/pause`);
      expect(pause.ok()).toBe(true);
      stoppedHumanState = (await pause.json()).human.state;
      expect(["paused", "game-over"]).toContain(stoppedHumanState.status);
      const stop = await page.request.post(`${host.url}/api/bots/stop`);
      expect(stop.ok()).toBe(true);
      current = await stop.json();
      expect(current.bots.every((slot) => slot.status === "idle")).toBe(true);
      expect(current.human.state).toEqual(stoppedHumanState);
      stopped = true;
    } else if (!stopped && current.human.state.gameOver) {
      await page.request.post(`${host.url}/api/commands/restart`);
    } else if (!stopped) {
      const direction = commandSequence++ % 2 === 0 ? "move-left" : "move-right";
      for (let step = 0; step < 3; step += 1) {
        await page.request.post(`${host.url}/api/commands/${direction}`);
      }
      await page.request.post(`${host.url}/api/commands/hard-drop`);
    }
    evidence = readLiveEvidence(resolve(runDirectory, "flaggo.db"));
    analysis = observedAnalysis(current);
    if (stopped && evidence.origins.size === 2 && evidence.sessions.size >= 11
      && analysis.sqlActions.length > 0 && analysis.outcome !== undefined
      && [...evidence.windows.values()].every((window) => window.values.size === 4
        && evidence.receipts.get(`${window.session}|${window.exposure}`) === window.interval)) break;
    await page.waitForTimeout(1_000);
  }
  const artifact = {
    sessions: [...evidence.sessions],
    origins: [...evidence.origins],
    windows: evidence.windows.size,
    sdkReceipts: evidence.receipts.size,
    copilotSqlActions: analysis.sqlActions,
    copilotCycle: { id: analysis.cycleId, outcome: analysis.outcome },
    copilotSqlCycles: analysis.sqlCycles,
    copilotCompletedCycles: analysis.completedCycles,
    sourceWindows: [...evidence.windows.values()].map((window) => ({
      ...window,
      values: Object.fromEntries(window.values),
      receiptInterval: evidence.receipts.get(`${window.session}|${window.exposure}`),
    })),
    activeExecutableDigest: current.executables.activeExecutable.executableDigest,
    analysis: current.controlLoop.analysis,
    services: current.services,
    backendFailures: current.backendFailures,
    note: "Automated commands exercise the human-facing composition; this is not human-preference validation.",
  };
  const path = testInfo.outputPath("live-evidence.json");
  await writeFile(path, JSON.stringify(artifact, null, 2), "utf8");
  await testInfo.attach("live-mixed-source-evidence", { path });
  expect([...evidence.origins].sort()).toEqual(["greedy", "human"]);
  expect(evidence.sessions.size).toBeGreaterThanOrEqual(11);
  expect(analysis.sqlActions.length, "A trusted SQL action must complete in the observed cycle.").toBeGreaterThan(0);
  for (const action of analysis.sqlActions) {
    expect(action.attributes["flaggo.analysis.query.digest"]).toMatch(/^sha256:[a-f0-9]{64}$/u);
    expect(action.attributes["flaggo.analysis.result.rows"]).toBeGreaterThanOrEqual(0);
    expect(action.attributes["flaggo.analysis.snapshot.watermark"]).toBeGreaterThanOrEqual(0);
    expect(action.attributes["flaggo.analysis.snapshot.cutoff"]).toBeDefined();
  }
  expect(["candidate", "no_candidate", "no_change"]).toContain(analysis.outcome);
  expect(evidence.windows.size).toBeGreaterThanOrEqual(4);
  for (const window of evidence.windows.values()) {
    expect(window.values.size).toBe(4);
    expect(window.dataset).toBe("live");
    expect(window.clock).toBe("wall");
    expect(window.active).toBeGreaterThanOrEqual(0);
    expect(window.active).toBeLessThanOrEqual(5_000);
    expect(window.end - window.start).toBeCloseTo(window.active, 6);
    expect(evidence.receipts.get(`${window.session}|${window.exposure}`)).toBe(window.interval);
  }
  expect(current.mode).toBe("live");
  expect(current.bots.every((slot) => slot.status === "idle")).toBe(true);
  expect(current.human.state).toEqual(stoppedHumanState);
  expect(current.services.every((service) => service.status === "healthy")).toBe(true);
  expect(current.backendFailures).toEqual([]);
});

function readLiveEvidence(path) {
  const database = new DatabaseSync(path, { readOnly: true });
  const origins = new Set();
  const sessions = new Set();
  const receipts = new Map();
  const windows = new Map();
  try {
    database.exec("PRAGMA busy_timeout = 5000");
    const rows = database.prepare(
      "SELECT DISTINCT signal_name, CASE WHEN instrumentation_scope = '@flaggo/sdk' "
      + "THEN json_extract(payload_json, '$.signal.payload.attributes') "
      + "ELSE json_extract(payload_json, '$.signal.payload.dataPoint.attributes') END AS attributes, "
      + "json_extract(payload_json, '$.signal.payload.dataPoint.value') AS value "
      + "FROM evidence_observations WHERE instrumentation_scope = 'tetris.experience' "
      + "OR (instrumentation_scope = '@flaggo/sdk' AND signal_name = 'flaggo.decision.received')",
    ).all();
    for (const row of rows) {
      const attributes = JSON.parse(row.attributes);
      if (row.signal_name === "flaggo.decision.received") {
        const session = attribute(attributes, "flaggo.correlation.session_id");
        const exposure = attribute(attributes, "flaggo.correlation.exposure_id");
        receipts.set(`${session}|${exposure}`, JSON.parse(attribute(attributes, "flaggo.result.json")));
      } else if (row.signal_name.startsWith("tetris.experience.")) {
        const pointValue = JSON.parse(row.value);
        const id = attribute(attributes, "tetris.experience.window.id");
        const session = attribute(attributes, "tetris.session.id");
        const origin = attribute(attributes, "tetris.player.origin");
        origins.add(origin);
        sessions.add(session);
        const window = windows.get(id) ?? {
          session, origin,
          exposure: attribute(attributes, "tetris.exposure.id"),
          interval: Number(attribute(attributes, "tetris.drop_interval.ms")),
          dataset: attribute(attributes, "tetris.experience.dataset"),
          clock: attribute(attributes, "tetris.experience.clock"),
          start: Number(attribute(attributes, "tetris.experience.window.start_ms")),
          end: Number(attribute(attributes, "tetris.experience.window.end_ms")),
          values: new Map(),
        };
        const value = Number(pointValue.value ?? pointValue.int64);
        expect(Number.isFinite(value)).toBe(true);
        if (window.values.has(row.signal_name)) expect(window.values.get(row.signal_name)).toBe(value);
        window.values.set(row.signal_name, value);
        if (row.signal_name === "tetris.experience.active_play_ms") window.active = value;
        windows.set(id, window);
      }
    }
    return { origins, sessions, receipts, windows };
  } finally {
    database.close();
  }
}

function attribute(attributes, key) {
  const attribute = attributes.find((value) => value.key === key);
  if (attribute === undefined) throw new Error(`Missing evidence attribute '${key}'.`);
  return attribute.value.value ?? attribute.value.int64;
}

function observedAnalysis(snapshot) {
  const byCycle = new Map();
  const completed = new Map();
  for (const event of snapshot.timeline) {
    if (event.name === "flaggo.analysis.cycle.completed"
      && ["candidate", "no_candidate", "no_change"].includes(event.outcome)) {
      completed.set(event.attributes["flaggo.analysis.cycle.id"], event.outcome);
    }
  }
  for (const action of snapshot.analysisActions.actions) {
    if (action.kind !== "evidence.sql" || action.status !== "completed" || action.outcome !== "success") continue;
    const actions = byCycle.get(action.cycleId) ?? [];
    actions.push(action);
    byCycle.set(action.cycleId, actions);
  }
  const observed = {
    sqlActions: [...byCycle.values()].flat(),
    sqlCycles: [...byCycle].map(([id, actions]) => ({ id, count: actions.length })),
    completedCycles: [...completed].map(([id, outcome]) => ({ id, outcome })),
  };
  for (const [cycleId, outcome] of completed) {
    const actions = byCycle.get(cycleId);
    if (actions?.length > 0) return { ...observed, sqlActions: actions, cycleId, outcome };
  }
  return observed;
}
