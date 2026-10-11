import { readFile, rm, writeFile } from "node:fs/promises";
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

test.afterEach(async ({}, testInfo) => {
  if (testInfo.status === testInfo.expectedStatus || runDirectory === undefined) return;
  const log = await readFile(resolve(runDirectory, "async-analysis.log"), "utf8");
  const path = testInfo.outputPath("async-analysis.log");
  await writeFile(path, log, "utf8");
  await testInfo.attach("analysis-diagnostics", { path });
});

test("live human and ten real-time players feed actual Copilot SQL analysis", async ({ page }, testInfo) => {
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
  let analysis = { origins: new Set() };
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
    const log = await readFile(resolve(runDirectory, "async-analysis.log"), "utf8");
    analysis = observedAnalysis(log);
    if (stopped && evidence.origins.size === 2 && evidence.sessions.size >= 11
      && analysis.origins.size === 2 && analysis.outcome !== undefined
      && [...evidence.windows.values()].every((window) => window.values.size === 4
        && evidence.receipts.get(`${window.session}|${window.exposure}`) === window.interval)) break;
    await page.waitForTimeout(1_000);
  }
  const artifact = {
    sessions: [...evidence.sessions],
    origins: [...evidence.origins],
    windows: evidence.windows.size,
    sdkReceipts: evidence.receipts.size,
    copilotSqlOrigins: [...analysis.origins],
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
  expect([...analysis.origins].sort(), "Actual run_sql results must contain both origins, not just contract guidance.")
    .toEqual(["greedy", "human"]);
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

function observedAnalysis(log) {
  const calls = new Map();
  const byCycle = new Map();
  const completed = new Map();
  const origins = new Set();
  for (const line of log.split(/\r?\n/u)) {
    if (!line.startsWith("{") || !line.endsWith("}")) continue;
    const row = JSON.parse(line);
    if (row["event.name"] === "flaggo.analysis.cycle.completed"
      && ["candidate", "no_candidate", "no_change"].includes(row["flaggo.operation.outcome"])) {
      completed.set(row["flaggo.analysis.cycle.id"], row["flaggo.operation.outcome"]);
    }
    const event = row.sessionEvent;
    if (event === undefined) continue;
    if (event.type === "tool.execution_start" && event.data?.toolName === "run_sql") {
      calls.set(event.data.toolCallId, row.cycleId);
    }
    if (event.type === "tool.execution_complete" && calls.has(event.data?.toolCallId)
      && event.data.success === true) {
      const result = JSON.stringify(event.data.result)?.replaceAll("\\", "");
      const cycleId = calls.get(event.data.toolCallId);
      const cycleOrigins = byCycle.get(cycleId) ?? new Set();
      for (const origin of ["human", "greedy"]) {
        if (result?.includes(`"${origin}"`)) {
          origins.add(origin);
          cycleOrigins.add(origin);
        }
      }
      byCycle.set(cycleId, cycleOrigins);
    }
  }
  const observed = {
    origins,
    sqlCycles: [...byCycle].map(([id, cycleOrigins]) => ({ id, origins: [...cycleOrigins] })),
    completedCycles: [...completed].map(([id, outcome]) => ({ id, outcome })),
  };
  for (const [cycleId, outcome] of completed) {
    const cycleOrigins = byCycle.get(cycleId);
    if (cycleOrigins?.size === 2) return { ...observed, origins: cycleOrigins, cycleId, outcome };
  }
  return observed;
}
