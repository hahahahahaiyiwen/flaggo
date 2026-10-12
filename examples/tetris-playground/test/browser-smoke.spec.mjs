import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";
import { createAnalysisActionsView } from "../dist/analysis-actions.js";

const exampleDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);
const eventClients = new Set();
const commands = [];
let inspectionSnapshot;
let baseUrl;
let server;

const snapshot = {
  revision: 4,
  observedAt: "2025-10-09T08:53:20.123Z",
  mode: "benchmark",
  bots: [],
  human: {
  state: {
    board: Array.from({ length: 20 }, (_, row) =>
      Array.from({ length: 10 }, (_, column) =>
        row === 19 && (column === 4 || column === 5) ? "O" : null
      )
    ),
    activePiece: "O",
    score: 24,
    lines: 1,
    level: 0,
    paused: false,
    gameOver: false,
    pendingRecoveryFailures: 0,
    dropObservation: {
      boardPressure: 0.1,
      currentLevel: 0,
      placementTimeMs: 700,
      recoveryFailures: 0,
      sessionId: "browser-smoke",
    },
    revision: 4,
    sessionId: "browser-smoke",
    status: "playing",
    dropInterval: {
      intervalMs: 200,
      source: "flaggo",
      status: "rule explore-fast-gravity",
    },
  },
  decision: {
    observedAt: "2025-10-09T08:53:20.123Z",
    sessionId: "browser-smoke",
    attributes: {
      board_pressure_mean_5s: 0.1,
      board_pressure_max_5s: 0.15,
      hard_drop_ratio_5s: 1,
      placement_time_mean_ms_5s: 700,
      pieces_locked_5s: 2,
      active_play_ms_5s: 5_000,
      challenge_balance_5s: 0.5,
      lines_cleared_5s: 1,
      game_over_count_5s: 0,
      current_drop_interval_ms: 800,
      session_id: "browser-smoke",
      exposure_id: "browser-exposure",
      _random: 0.25,
    },
    result: 200,
    source: "flaggo",
    status: "rule explore-fast-gravity",
    rule: "explore-fast-gravity",
    contractDigest: `sha256:${"0".repeat(64)}`,
    executableDigest: `sha256:${"1".repeat(64)}`,
    latencyMs: 4.5,
    traceId: "0123456789abcdef0123456789abcdef",
  },
  experience: { activePlayMs: 5_000, balancedPlayMs: 2_500, linesCleared: 1, gameOvers: 0 },
  totals: { activePlayMs: 5_000, balancedPlayMs: 2_500, linesCleared: 1, gameOvers: 0 },
  },
  executables: {
    status: "ready",
    contractName: "tetris.dropInterval",
    contractDigest: `sha256:${"0".repeat(64)}`,
    activeExecutable: {
      contractDigest: `sha256:${"0".repeat(64)}`,
      executableDigest: `sha256:${"1".repeat(64)}`,
      lifecycleState: "active",
      stateVersion: 1,
      createdAt: "2025-10-09T08:50:00.123Z",
      activatedAt: "2025-10-09T08:50:01.123Z",
      executable: {
        contractDigest: `sha256:${"0".repeat(64)}`,
        kind: "rules",
        rules: [{
          name: "retain-current-without-observations",
          when: {
            expression: "attributes.active_play_ms_5s == 0.0 || attributes.pieces_locked_5s == 0",
          },
          return: { expression: "attributes.current_drop_interval_ms" },
        }, {
          name: "explore-fast-gravity",
          when: { expression: "attributes._random < 0.5" },
          return: { value: 200 },
        }, {
          name: "explore-slow-gravity",
          when: { expression: "true" },
          return: { value: 1_000 },
        }],
      },
      provenance: { source: "authored" },
    },
    candidates: [{
      contractDigest: `sha256:${"0".repeat(64)}`,
      executableDigest: `sha256:${"2".repeat(64)}`,
      lifecycleState: "candidate",
      stateVersion: 0,
      createdAt: "2025-10-09T08:53:18.123Z",
      executable: {
        contractDigest: `sha256:${"0".repeat(64)}`,
        kind: "rules",
        rules: [{
          name: "retain-current-without-observations",
          when: {
            expression: "attributes.active_play_ms_5s == 0.0 || attributes.pieces_locked_5s == 0",
          },
          return: { expression: "attributes.current_drop_interval_ms" },
        }, {
          name: "learned-low-pressure",
          when: { expression: "attributes.board_pressure_mean_5s < 0.15" },
          return: { value: 200 },
        }, {
          name: "learned-other-context",
          when: { expression: "true" },
          return: { value: 1_000 },
        }],
      },
      provenance: {
        source: "async-analysis",
        cycleId: "cycle-1",
      },
    }],
    observedAt: "2025-10-09T08:53:20.123Z",
  },
  demo: {
    stage: "idle",
    detail: "Ready to compare authored and learned gravity on matched games.",
  },
  services: [
    ["flaggo-contract-service", "Contract Service"],
    ["flaggo-decision-service", "Decision Service"],
    ["flaggo-otel-ingestion", "OTel Ingestion"],
    ["flaggo-evidence-materializer", "Evidence Materializer"],
    ["flaggo-async-analysis", "Async Analysis"],
  ].map(([name, displayName], index) => ({
    name,
    displayName,
    process: "running",
    readiness: index < 3 ? "ready" : "not-applicable",
    status: "healthy",
    detail: "current telemetry",
    lastSignalAt: "2025-10-09T08:53:20.123Z",
  })),
  evidence: {
    inboxRetainedBatches: 2,
    inboxRetainedBytes: 4_096,
    checkpointBatchId: "42",
    pendingBatches: 0,
    evidenceFreshnessMs: 200,
    evidenceStoreObservations: 8,
    evidenceStoreProvenance: 8,
    batchesRead: 2,
    observationsCreated: 8,
    duplicateObservations: 0,
    conflictsCreated: 0,
    diagnosticsCreated: 0,
    observedAt: "2025-10-09T08:53:20.123Z",
    stale: false,
  },
  controlLoop: {
    analysis: {
      observedAt: "2025-10-09T08:53:20.123Z",
      name: "flaggo.analysis.cycle.completed",
      outcome: "candidate",
      status: "success",
      cycleId: "cycle-1",
      candidateDigest: `sha256:${"2".repeat(64)}`,
    },
    candidate: {
      observedAt: "2025-10-09T08:53:20.123Z",
      name: "flaggo.candidate.admitted",
      outcome: "success",
      status: "success",
      candidateDigest: `sha256:${"2".repeat(64)}`,
    },
    activation: {
      observedAt: "2025-10-09T08:53:20.123Z",
      name: "flaggo.activation.completed",
      outcome: "success",
      status: "success",
      executableDigest: `sha256:${"1".repeat(64)}`,
    },
  },
  activity: { status: "current", lastSuccessfulPollAt: "2025-10-09T08:53:20.123Z" },
  timeline: [{
    id: "analysis-noise",
    observedAt: "2025-10-09T08:53:21.123Z",
    phase: "analysis",
    service: "flaggo-async-analysis",
    signal: "log",
    name: "flaggo.analysis.cycle.completed",
    title: "analysis cycle completed: not_eligible",
    outcome: "not_eligible",
    attributes: {
      "flaggo.operation.outcome": "not_eligible",
    },
  }, {
    id: "candidate",
    observedAt: "2025-10-09T08:53:20.123Z",
    phase: "candidate",
    service: "flaggo-contract-service",
    signal: "log",
    name: "flaggo.candidate.admitted",
    title: "candidate admitted: success",
    outcome: "success",
    attributes: {},
  }, {
    id: "activation",
    observedAt: "2025-10-09T08:53:19.123Z",
    phase: "activation",
    service: "flaggo-contract-service",
    signal: "log",
    name: "flaggo.activation.completed",
    title: "activation completed: success",
    outcome: "success",
    attributes: {},
  }],
  metrics: [{
    name: "flaggo_contract_operations_total",
    service: "flaggo-contract-service",
    value: 7,
    labels: {},
  }, {
    name: "flaggo_decision_requests_total",
    service: "flaggo-decision-service",
    value: 11,
    labels: {},
  }],
  backendFailures: [],
  analysisActions: {
    status: "current", lastSuccessfulPollAt: "2025-10-09T08:53:20.123Z",
    observations: [], actions: [], limited: false, observationLimit: 400, actionLimit: 200,
  },
  urls: {
    grafana: "http://127.0.0.1:3210",
  },
};

snapshot.executables.history = [
  snapshot.executables.activeExecutable,
  ...snapshot.executables.candidates,
  {
    ...snapshot.executables.activeExecutable,
    executableDigest: `sha256:${"3".repeat(64)}`,
    lifecycleState: "inactive",
  },
];

test.beforeAll(async () => {
  server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (request.method === "GET" && url.pathname === "/") {
      return sendFile(response, "public/index.html", "text/html");
    }
    if (request.method === "GET" && url.pathname === "/styles.css") {
      return sendFile(response, "public/styles.css", "text/css");
    }
    if (request.method === "GET" && url.pathname === "/app.js") {
      return sendFile(response, "dist/client.js", "text/javascript");
    }
    if (request.method === "GET" && url.pathname === "/analysis-actions.js") {
      return sendFile(response, "dist/analysis-actions.js", "text/javascript");
    }
    if (request.method === "GET" && url.pathname === "/api/events") {
      response.writeHead(200, {
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
        "Content-Type": "text/event-stream",
      });
      eventClients.add(response);
      response.write(`event: state\ndata: ${JSON.stringify(snapshot)}\n\n`);
      request.once("close", () => eventClients.delete(response));
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/analysis/actions") {
      response.writeHead(200, { "Content-Type": "application/json" });
      const selection = Object.fromEntries(url.searchParams);
      const view = inspectionSnapshot.analysisActions;
      response.end(JSON.stringify(createAnalysisActionsView({
        logs: [], previous: view, selection, now: new Date(inspectionSnapshot.observedAt),
        ...(view.error === undefined ? {} : { error: view.error }),
      })));
      return;
    }
    if (request.method === "POST") {
      commands.push(url.pathname);
      response.writeHead(202, { "Content-Type": "application/json" });
      response.end(JSON.stringify(
        url.pathname.includes("/api/executables/")
          ? snapshot
          : {},
      ));
      return;
    }
    response.writeHead(404).end();
  });
  server.listen(0, "127.0.0.1");
  await new Promise((resolvePromise) => server.once("listening", resolvePromise));
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Fixture server did not bind an IP socket.");
  }
  baseUrl = `http://127.0.0.1:${address.port}`;
});

test.afterAll(async () => {
  for (const client of eventClients) client.end();
  await new Promise((resolvePromise, reject) => {
    server.close((error) => {
      if (error === undefined) resolvePromise();
      else reject(error);
    });
  });
});

test.beforeEach(() => {
  inspectionSnapshot = snapshot;
});

test("continues showing newest activity after the visible history fills", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const filter = page.getByLabel("Not eligible", { exact: true });
  await expect(filter).not.toBeChecked();
  const base = Date.parse(snapshot.observedAt);
  const events = Array.from({ length: 75 }, (_, index) => ({
    id: `rolling-${index}`,
    observedAt: new Date(base + index * 1_000).toISOString(),
    phase: "analysis",
    service: "flaggo-async-analysis",
    signal: "log",
    name: "flaggo.analysis.cycle.completed",
    title: `Analysis ${index}`,
    outcome: "no_candidate",
    attributes: {},
  }));
  publishState({ ...snapshot, timeline: [...events].reverse() });
  await expect(page.locator(".timeline-event")).toHaveCount(60);
  await expect(page.locator(".timeline-event").first()).toHaveAttribute("data-event-id", "rolling-74");
  await expect(page.locator(".timeline-event").last()).toHaveAttribute("data-event-id", "rolling-15");
  for (let index = 75; index < 78; index += 1) {
    events.push({
      ...events[0],
      id: `rolling-${index}`,
      observedAt: new Date(base + index * 1_000).toISOString(),
      title: `Analysis ${index}`,
    });
    publishState({ ...snapshot, timeline: events });
    await expect(page.locator(".timeline-event")).toHaveCount(60);
    await expect(page.locator(".timeline-event").first())
      .toHaveAttribute("data-event-id", `rolling-${index}`);
    await expect(page.locator(".timeline-event").last())
      .toHaveAttribute("data-event-id", `rolling-${index - 59}`);
  }
  const noOp = {
    ...events[0],
    id: "newest-not-eligible",
    observedAt: new Date(base + 100_000).toISOString(),
    title: "analysis cycle completed: not_eligible",
    outcome: "not_eligible",
  };
  publishState({ ...snapshot, timeline: [...events, noOp] });
  await expect(page.locator(".timeline-event").first()).toHaveAttribute("data-event-id", "rolling-77");
  await filter.check();
  await expect(page.locator(".timeline-event").first())
    .toHaveAttribute("data-event-id", "newest-not-eligible");
  await filter.uncheck();
  await expect(page.locator(".timeline-event").first()).toHaveAttribute("data-event-id", "rolling-77");
});

test("action inspection keeps selection, expanded details, and focus during gameplay SSE", async ({ page }) => {
  const before = commands.length;
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const action = {
    key: "cycle-1-attempt-1-action-1",
    contractDigest: `sha256:${"0".repeat(64)}`, contractName: "tetris.dropInterval",
    cycleId: "cycle-1", attemptId: "attempt-1",
    actionId: "00000000-0000-4000-8000-000000000001", kind: "evidence.sql",
    startedAt: "2025-10-09T08:53:19.123Z", latestObservedAt: "2025-10-09T08:53:20.123Z",
    completedAt: "2025-10-09T08:53:20.123Z", status: "completed", outcome: "success",
    attributes: {
      "flaggo.analysis.action.duration_ms": 14, "flaggo.analysis.result.rows": 3,
      "flaggo.analysis.result.bytes": 250, "flaggo.analysis.query.digest": `sha256:${"4".repeat(64)}`,
      "flaggo.analysis.artifact.digest": `sha256:${"5".repeat(64)}`,
      "flaggo.analysis.snapshot.watermark": 42, "flaggo.analysis.snapshot.cutoff": "2025-10-09T08:53:00Z",
    },
    traceId: "0123456789abcdef0123456789abcdef",
  };
  const otherAttempt = { ...action, key: "cycle-1-attempt-2", attemptId: "attempt-2",
    actionId: "00000000-0000-4000-8000-000000000002", kind: "session.run",
    completedAt: undefined, status: "completion not observed", outcome: undefined, attributes: {} };
  const otherCycle = { ...action, key: "cycle-2", cycleId: "cycle-2" };
  const view = { ...snapshot.analysisActions, actions: [action, otherAttempt, otherCycle] };
  const initial = { ...snapshot, analysisActions: view };
  publishState(initial);
  await expect(page.locator(".analysis-action")).toHaveCount(1);
  await expect(page.locator("#analysis-latest-action")).toContainText("evidence.sql");
  const row = page.locator(".analysis-action").first();
  const summary = row.locator("summary");
  await summary.click();
  await expect(row).toHaveAttribute("open", "");
  await expect(row).toContainText("Duration (ms)");
  await expect(row).toContainText("Rows");
  await expect(row).toContainText("250");
  await expect(row).toContainText(`sha256:${"4".repeat(64)}`);
  await expect(row).toContainText("Watermark");
  await expect(row.locator(".action-trace")).toHaveAttribute("href", /explore.*tempo/u);
  await summary.focus();
  await page.evaluate(() => {
    window.inspectedActionNode = document.querySelector(".analysis-action");
    window.inspectedSummaryNode = document.activeElement;
  });
  for (let index = 0; index < 4; index += 1) {
    publishState({ ...initial, revision: 100 + index,
      human: { ...snapshot.human, state: { ...snapshot.human.state, score: 100 + index } } });
    await expect(page.locator("#score")).toHaveText(String(100 + index));
  }
  expect(await page.evaluate(() =>
    window.inspectedActionNode === document.querySelector(".analysis-action")
    && window.inspectedSummaryNode === document.activeElement)).toBe(true);
  await expect(row).toHaveAttribute("open", "");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press(" ");
  expect(commands.length).toBe(before);
  await page.locator("#analysis-attempt").selectOption("attempt-2");
  await expect(page.locator(".analysis-action")).toContainText("completion not observed");
  publishState({ ...initial, analysisActions: { ...view, actions: [otherCycle, action, otherAttempt] } });
  await expect(page.locator("#analysis-attempt")).toHaveValue("attempt-2");
  await expect(page.locator("#analysis-cycle")).toHaveValue(JSON.stringify([action.contractDigest, "cycle-1"]));
  await expect(page.locator(".analysis-action")).toContainText("session.run");
});

test("refreshed older actions survive capped SSE history and inspection outages", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const action = {
    contractDigest: `sha256:${"0".repeat(64)}`, contractName: "tetris.dropInterval",
    cycleId: "older-cycle", attemptId: "older-attempt",
    actionId: "00000000-0000-4000-8000-000000000004", kind: "evidence.sql",
    latestObservedAt: snapshot.observedAt, completedAt: snapshot.observedAt,
    status: "completed", outcome: "success", attributes: {
      "flaggo.analysis.action.duration_ms": 5, "flaggo.analysis.result.rows": 3,
    },
  };
  publishState({ ...snapshot, analysisActions: { ...snapshot.analysisActions, actions: [action] } });
  const scoped = inspectionSnapshot.analysisActions;
  let unavailable = false;
  await page.route("**/api/analysis/actions?*", (route) => route.fulfill({
    contentType: "application/json",
    body: JSON.stringify(unavailable ? {
      ...scoped, observations: [], actions: [], status: "stale",
      error: "Loki unavailable; retaining latest observed facts.",
      lastSuccessfulPollAt: undefined,
    } : scoped),
  }));
  await expect(page.locator("#analysis-cycle")).toHaveValue(
    JSON.stringify([action.contractDigest, action.cycleId]));
  const newer = Array.from({ length: 400 }, (_, index) => ({
    ...action, cycleId: "newer-cycle", attemptId: "newer-attempt",
    actionId: `10000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
    latestObservedAt: new Date(Date.parse(snapshot.observedAt) + index + 1).toISOString(),
    completedAt: new Date(Date.parse(snapshot.observedAt) + index + 1).toISOString(),
  }));
  publishState({ ...snapshot, analysisActions: { ...snapshot.analysisActions, actions: newer } });
  expect(inspectionSnapshot.analysisActions.observations).toHaveLength(400);
  expect(inspectionSnapshot.analysisActions.actions).toHaveLength(200);
  await expect(page.locator(".analysis-action")).toHaveCount(0);
  await page.locator("#analysis-refresh").click();
  const row = page.locator(`[data-action-id="${action.actionId}"]`);
  await expect(row).toContainText("success");
  await row.locator("summary").click();
  await row.locator("summary").focus();
  await page.evaluate(() => {
    window.inspectedActionNode = document.querySelector(".analysis-action");
    window.inspectedSummaryNode = document.activeElement;
  });
  publishState({ ...inspectionSnapshot, revision: 100,
    human: { ...snapshot.human, state: { ...snapshot.human.state, score: 101 } } });
  await expect(page.locator("#score")).toHaveText("101");
  await expect(row).toHaveAttribute("open", "");
  expect(await page.evaluate(() =>
    window.inspectedActionNode === document.querySelector(".analysis-action")
    && window.inspectedSummaryNode === document.activeElement)).toBe(true);
  unavailable = true;
  await page.locator("#analysis-refresh").click();
  await expect(page.locator("#analysis-action-status")).toContainText("Loki unavailable");
  await expect(row).toContainText("success");
  await expect(row).toHaveAttribute("open", "");
  unavailable = false;
  await page.locator("#analysis-refresh").click();
  await expect(page.locator("#analysis-action-status")).toContainText("current");
  await page.locator("#analysis-cycle").selectOption(
    JSON.stringify([action.contractDigest, "newer-cycle"]));
  await expect(page.locator(`[data-action-id="${action.actionId}"]`)).toHaveCount(0);
});

test("action telemetry partial, conflicting, outage and recovery states remain factual", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const action = {
    key: "terminal-only", contractDigest: `sha256:${"0".repeat(64)}`, contractName: "tetris.dropInterval",
    cycleId: "cycle-terminal", attemptId: "attempt-terminal",
    actionId: "00000000-0000-4000-8000-000000000003", kind: "artifact.write",
    latestObservedAt: snapshot.observedAt, completedAt: snapshot.observedAt,
    status: "completed", outcome: "failed", attributes: { "flaggo.analysis.action.duration_ms": 5 },
  };
  const view = { ...snapshot.analysisActions, actions: [action], limited: true };
  publishState({ ...snapshot, analysisActions: view });
  await page.locator(".analysis-action summary").click();
  await expect(page.locator(".analysis-action")).toContainText("Start not observed");
  await expect(page.locator("#analysis-action-status")).toContainText("limit reached");
  const stale = { ...view, status: "stale", error: "Loki unavailable; retained facts." };
  publishState({ ...snapshot, analysisActions: stale });
  await expect(page.locator("#analysis-action-status")).toContainText("Loki unavailable");
  await expect(page.locator(".analysis-action")).toContainText("failed");
  await expect(page.locator(".analysis-action")).toHaveAttribute("open", "");
  const conflict = { ...action, kind: "unknown/conflicting", status: "unknown/conflicting",
    outcome: undefined, completedAt: undefined, attributes: {} };
  publishState({ ...snapshot, analysisActions: { ...view, actions: [conflict] } });
  await expect(page.locator("#analysis-action-status")).toContainText("current");
  await expect(page.locator(".analysis-action")).toContainText("unknown/conflicting");
  await expect(page.locator(".analysis-action")).not.toContainText("success");
  await page.route("**/api/analysis/actions?*", (route) =>
    route.fulfill({ status: 503, body: "PRIVATE backend error" }));
  await page.locator("#analysis-refresh").click();
  await expect(page.locator("#analysis-action-status")).toContainText("backend unavailable");
  await expect(page.locator(".analysis-action")).toContainText("unknown/conflicting");
  await expect(page.locator("#analysis-action-status")).not.toContainText("PRIVATE");
  await page.unroute("**/api/analysis/actions?*");
  await page.locator("#analysis-refresh").click();
  await expect(page.locator("#analysis-action-status")).toContainText("current");
});

test("renders split control lanes with health, executables, pipeline, and filters", async ({
  page,
}) => {
  await page.goto(baseUrl);

  await expect(page.locator("#connection")).toHaveText("Live");
  await expect(page.locator("#board .cell")).toHaveCount(200);
  await expect(page.locator("#score")).toHaveText("24");
  await expect(page.locator(".decision-result span")).toHaveText("200");
  await expect(page.locator(".application-pane")).toBeVisible();
  await expect(page.locator(".cycle-lane")).toHaveCount(3);
  await expect(page.locator(".health-pill")).toHaveCount(5);
  const boardBox = await page.locator(".board-wrap").boundingBox();
  const controlsBox = await page.locator(".controls").boundingBox();
  const applicationBox = await page.locator(".application-card").boundingBox();
  const demoBox = await page.locator(".demo-card").boundingBox();
  expect(boardBox).not.toBeNull();
  expect(controlsBox).not.toBeNull();
  expect(applicationBox).not.toBeNull();
  expect(demoBox).not.toBeNull();
  expect(boardBox.y + boardBox.height).toBeLessThanOrEqual(controlsBox.y);
  expect(demoBox.height).toBeLessThan(applicationBox.height * 0.4);
  await expect(page.locator("#evidence")).toContainText("OTLP Inbox");
  await expect(page.locator("#evidence")).toContainText("EvidenceStore");
  await expect(page.locator(".telemetry-board")).toBeVisible();
  await expect(page.locator(".live-feed-indicator")).toHaveText(
    "Live telemetry",
  );
  await expect(page.locator(".executable-list-heading")).toContainText(
    "Status",
  );
  await expect(page.locator("#executables .executable-detail")).toHaveCount(0);
  await expect(page.locator("#executables .executable-option")).toHaveCount(3);
  await expect(
    page.locator("#executables .executable-option").first()
      .locator(".executable-state"),
  ).toHaveText("Active");
  await page.locator("#executables .executable-option").nth(2).click();
  await expect(page.locator("#executable-title")).toHaveText("Inactive executable");
  await expect(page.locator("[data-executable-activate]")).toHaveCount(0);
  await page.locator("#executable-close").click();
  await page.locator("#executables .executable-option").nth(1).click();
  await expect(page.locator("#executable-dialog")).toBeVisible();
  await expect(page.locator("#executable-title")).toHaveText(
    "Candidate executable",
  );
  await expect(page.locator("#executable-detail")).toContainText(
    "learned-low-pressure",
  );
  await page.locator("[data-executable-activate]").click();
  await expect.poll(() =>
    commands.some((path) => path.includes("/api/executables/"))
  ).toBe(true);
  await expect(page.locator("#executable-title")).toHaveText(
    "Active executable",
  );
  await page.locator("#executable-close").click();
  await expect(page.locator("#executable-dialog")).not.toBeVisible();

  await expect(page.locator("#timeline")).not.toContainText("not_eligible");
  await expect(page.getByLabel("Not eligible", { exact: true })).not.toBeChecked();
  await expect(page.locator(".timeline-event")).toHaveCount(2);
  await expect(page.locator(".timeline-event").nth(0)).toContainText(
    "candidate admitted",
  );
  await expect(page.locator(".timeline-event").nth(1)).toContainText(
    "activation completed",
  );
  const rollingSnapshot = {
    ...snapshot,
    revision: snapshot.revision + 1,
    observedAt: "2025-10-09T08:53:22.123Z",
    timeline: [...snapshot.timeline, {
      id: "analysis-newest",
      observedAt: "2025-10-09T08:53:22.123Z",
      phase: "analysis",
      service: "flaggo-async-analysis",
      signal: "log",
      name: "flaggo.analysis.cycle.completed",
      title: "analysis cycle completed: candidate",
      outcome: "candidate",
      attributes: {},
    }],
  };
  publishState(rollingSnapshot);
  await expect(page.locator(".timeline-event").first()).toContainText(
    "analysis cycle completed: candidate",
  );
  await expect(page.locator(".timeline-event").first()).toHaveClass(
    /arriving/u,
  );
  await expect(page.locator(".timeline-event")).toHaveCount(3);
  await expect(page.locator(".timeline-event").nth(1)).toContainText(
    "candidate admitted",
  );
  await expect(page.locator(".timeline-event").nth(2)).toContainText(
    "activation completed",
  );
  await page.locator("#filter-not-eligible").check();
  await expect(page.locator("#timeline")).toContainText("not_eligible");
  await expect(page.locator(".timeline-event").first()).toContainText(
    "analysis cycle completed: candidate",
  );
  await page.locator('[data-analysis-phase="candidate"]').uncheck();
  await expect(page.locator("#timeline")).not.toContainText("candidate admitted");

  await page.locator('[data-service-name="flaggo-otel-ingestion"]').click();
  await expect(page.locator("#health-dialog")).toBeVisible();
  await expect(page.locator("#health-title")).toHaveText("OTel Ingestion");
  await expect(page.locator("#health-detail")).toContainText("Ready");
  await page.locator("#health-close").click();

  await expect(page.locator("#grafana-link")).toHaveAttribute(
    "href",
    snapshot.urls.grafana,
  );

  await page.locator('[data-command="hard-drop"]').click();
  await expect.poll(() =>
    commands.includes("/api/commands/hard-drop")
  ).toBe(true);
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() =>
    commands.includes("/api/commands/move-left")
  ).toBe(true);
  await page.locator("#demo-run").click();
  await expect.poll(() => commands.includes("/api/demo/run")).toBe(true);
});

test("shows backlog, superseded and failed analysis, failed activation, and recovery", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const events = [
    ["superseded", "analysis", "flaggo.analysis.cycle.completed", "superseded"],
    ["analysis-failure", "analysis", "flaggo.analysis.cycle.failed", "recoverable"],
    ["activation-failure", "activation", "flaggo.activation.rejected", "conflict"],
  ].map(([id, phase, name, outcome], index) => ({
    id, phase, name, outcome,
    observedAt: new Date(Date.parse(snapshot.observedAt) + index * 1_000).toISOString(),
    service: phase === "analysis" ? "flaggo-async-analysis" : "flaggo-contract-service",
    signal: "log",
    title: `${name}: ${outcome}`,
    attributes: {},
  }));
  publishState({
    ...snapshot,
    evidence: { ...snapshot.evidence, pendingBatches: 12, evidenceFreshnessMs: 30_000, stale: true },
    timeline: events,
  });
  await expect(page.locator("#evidence")).toContainText("12 pending");
  await expect(page.locator("#evidence")).toContainText("30.0s freshness");
  await expect(page.locator("#timeline")).toContainText("superseded");
  await expect(page.locator("#timeline")).toContainText("flaggo.analysis.cycle.failed");
  await expect(page.locator("#timeline")).toContainText("flaggo.activation.rejected");
  await expect(page.locator("#executables .executable-option").first()
    .locator(".executable-state")).toHaveText("Active");
  publishState({
    ...snapshot,
    timeline: [...events, {
      ...events[1], id: "analysis-recovery",
      observedAt: new Date(Date.parse(snapshot.observedAt) + 10_000).toISOString(),
      name: "flaggo.analysis.cycle.completed", outcome: "no_candidate",
      title: "analysis cycle completed: no_candidate",
    }],
  });
  await expect(page.locator("#evidence")).toContainText("0 pending");
  await expect(page.locator(".timeline-event").first()).toContainText("no_candidate");
  await expect(page.locator("#timeline")).toContainText("flaggo.analysis.cycle.failed");
});

test("executable clicks and open details survive gameplay updates and catalogue recovery", async ({ page }) => {
  await page.goto(baseUrl);
  const row = page.locator(".executable-option").nth(1);
  await expect(row).toBeVisible();
  const pressedRow = await row.elementHandle();
  const box = await row.boundingBox();
  const publishRevision = (revision, executables = snapshot.executables) => publishState({
    ...snapshot,
    revision,
    human: { ...snapshot.human, state: { ...snapshot.human.state, score: revision } },
    executables: { ...executables, observedAt: new Date().toISOString() },
  });
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  publishRevision(101);
  await expect(page.locator("#score")).toHaveText("101");
  expect(await pressedRow.evaluate((element) => element.isConnected)).toBe(true);
  await page.mouse.up();
  await expect(page.locator("#executable-dialog")).toBeVisible();
  await expect(page.locator("#executable-title")).toHaveText("Candidate executable");
  const rule = await page.locator("#executable-detail .rule-card").first().elementHandle();
  const activate = page.locator("[data-executable-activate]");
  await activate.focus();
  publishRevision(102);
  await expect(page.locator("#score")).toHaveText("102");
  expect(await rule.evaluate((element) => element.isConnected)).toBe(true);
  await expect(activate).toBeFocused();
  publishRevision(103, { ...snapshot.executables, status: "unavailable", error: "Management read timeout" });
  await expect(page.locator(".catalog-error")).toContainText("timeout");
  await expect(page.locator("#executable-dialog")).toBeVisible();
  await expect(page.locator("#executable-detail")).toContainText("learned-low-pressure");
  await expect(activate).toBeDisabled();
  publishRevision(104);
  await expect(page.locator(".catalog-error")).toHaveCount(0);
  await expect(activate).toBeEnabled();
  const candidate = snapshot.executables.candidates[0];
  const active = { ...candidate, lifecycleState: "active", stateVersion: candidate.stateVersion + 1 };
  publishRevision(105, {
    ...snapshot.executables,
    activeExecutable: active,
    candidates: [],
    history: [active, { ...snapshot.executables.activeExecutable, lifecycleState: "inactive" }],
  });
  await expect(page.locator("#executable-title")).toHaveText("Active executable");
  await expect(activate).toHaveCount(0);
});

test("renders pending, successful, and inconclusive measured comparisons", async ({
  page,
}) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const baseline = {
    executableDigest: snapshot.executables.activeExecutable.executableDigest,
    episodes: 3,
    plannedPlayMs: 360_000,
    activePlayMs: 300_000,
    challengeBalance: 0.5,
    linesCleared: 6,
    linesPerMinute: 1.2,
    gameOvers: 1,
  };
  const learned = {
    executableDigest: snapshot.executables.candidates[0].executableDigest,
    episodes: 3,
    plannedPlayMs: 360_000,
    activePlayMs: 360_000,
    challengeBalance: 0.75,
    linesCleared: 18,
    linesPerMinute: 3,
    gameOvers: 0,
  };
  const comparisonSnapshot = {
    ...snapshot,
    revision: snapshot.revision + 1,
    demo: {
      stage: "verifying",
      detail: "Measuring the activated policy on the same held-out games.",
      baseline,
    },
  };
  publishState(comparisonSnapshot);
  await expect(page.locator(".comparison")).toContainText("50.0% balanced");
  await expect(page.locator(".comparison")).toContainText("1.2 lines/min");
  await expect(page.locator(".comparison")).toContainText("2/3 survived");
  await expect(page.locator(".comparison")).toContainText("Awaiting evaluation");
  await expect(page.locator("#demo-run")).toBeDisabled();

  publishState({
    ...comparisonSnapshot,
    revision: snapshot.revision + 2,
    demo: {
      stage: "completed",
      detail: "Held-out balance improved 25.0 percentage points; guardrails passed.",
      baseline,
      learned,
      balanceImprovement: 0.25,
    },
  });

  await expect(page.locator(".demo-summary.completed")).toBeVisible();
  await expect(page.locator(".comparison")).toContainText("75.0% balanced");
  await expect(page.locator(".comparison")).toContainText("3.0 lines/min");
  await expect(page.locator(".comparison")).toContainText("3/3 survived");
  await expect(page.locator(".comparison code")).toHaveCount(2);
  await expect(page.locator("#demo-run")).toBeEnabled();

  publishState({
    ...comparisonSnapshot,
    revision: snapshot.revision + 3,
    demo: {
      stage: "failed",
      detail: "Inconclusive comparison: balance improved 2.0 percentage points; at least 5 are required.",
      baseline,
      learned: { ...learned, challengeBalance: 0.52 },
    },
  });
  await expect(page.locator(".demo-summary.failed")).toContainText("Inconclusive");
  await expect(page.locator(".demo-summary.completed")).toHaveCount(0);
  await expect(page.locator(".comparison")).toContainText("52.0% balanced");
  await expect(page.locator("#demo-cancel")).toBeDisabled();
});

test("shows retained activity with a stale indicator and recovers", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator(".timeline-event")).toHaveCount(2);
  publishState({
    ...snapshot,
    activity: {
      status: "stale",
      lastSuccessfulPollAt: snapshot.activity.lastSuccessfulPollAt,
      error: "loki: HTTP 503",
    },
    backendFailures: [{ source: "loki", message: "HTTP 503", observedAt: snapshot.observedAt }],
  });
  await expect(page.locator("#analysis-feed-status")).toContainText("Stale / partial telemetry");
  await expect(page.locator("#analysis-feed-status")).toHaveAttribute("title", /HTTP 503/);
  await expect(page.locator(".timeline-event")).toHaveCount(2);
  await expect(page.locator("#backend-failures")).toContainText("HTTP 503");
  publishState(snapshot);
  await expect(page.locator("#analysis-feed-status")).toHaveText("Live telemetry");
  await expect(page.locator("#backend-failures")).toBeEmpty();
  await expect(page.locator(".timeline-event")).toHaveCount(2);
});

test("live gallery preserves the human board, keyboard ownership, and duration-weighted reporting", async ({ page }) => {
  await page.goto(baseUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  const live = {
    ...snapshot,
    mode: "live",
    bots: [2_000, 6_000, 12_000, 2_000, 6_000, 12_000, 2_000, 6_000, 12_000, 20_000].map((activePlayMs, index) => ({
      id: `bot-${index + 1}`, status: "running", episode: 1,
      totals: {
        activePlayMs, balancedPlayMs: index === 9 ? 11_600 : [200, 1_800, 9_600][index % 3],
        linesCleared: 1, gameOvers: 0,
      },
      session: {
        ...snapshot.human,
        state: {
          ...snapshot.human.state,
          sessionId: `bot-session-${index + 1}`,
          dropInterval: { ...snapshot.human.state.dropInterval, intervalMs: 1_000,
            executableDigest: snapshot.executables.activeExecutable.executableDigest },
          pendingDropInterval: { intervalMs: 200, source: "flaggo", status: "queued",
            executableDigest: snapshot.executables.candidates[0].executableDigest },
        },
        decision: { ...snapshot.human.decision, sessionId: `bot-session-${index + 1}`, result: 200 },
      },
    })),
  };
  publishState(live);
  const expectBotWindow = async (ids, range) => {
    await expect(page.locator(".bot-card")).toHaveCount(ids.length);
    await expect.poll(() => page.locator(".bot-card")
      .evaluateAll((cards) => cards.map((card) => card.getAttribute("data-bot-id")))).toEqual(ids);
    await expect(page.locator("#bots-range")).toHaveText(range);
    await expect(page.locator(".bot-board .cell")).toHaveCount(ids.length * 200);
  };
  await expectBotWindow(["bot-1", "bot-2", "bot-3"], "Bots 1-3 of 10");
  await expect(page.locator("#bots-previous")).toBeDisabled();
  await expect(page.locator("#bots-next")).toBeEnabled();
  await expect(page.locator("#session-select option")).toHaveCount(11);
  await expect(page.locator("#benchmark-panel")).toBeHidden();
  await expect(page.locator("#population-results")).toContainText("58.0%");
  await expect(page.locator("#population-results")).toContainText("Human");
  const humanBox = await page.locator("#board").boundingBox();
  const botBox = await page.locator(".bot-board").first().boundingBox();
  expect(humanBox.height).toBeGreaterThan(botBox.height * 2);
  await page.locator("#session-select").selectOption("bot-10");
  await expect(page.locator(".decision-result span")).toHaveText("1000");
  await expect(page.locator("#decision")).toContainText("Queued");
  await expect(page.locator("#decision")).toContainText("200 ms");
  const commandsBeforeBrowsing = commands.length;
  await page.locator("#bots-next").click();
  await expectBotWindow(["bot-2", "bot-3", "bot-4"], "Bots 2-4 of 10");
  await page.locator("#bots-next").focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expectBotWindow(["bot-3", "bot-4", "bot-5"], "Bots 3-5 of 10");
  publishState({
    ...live,
    bots: live.bots.map((slot) => slot.id === "bot-4" ? { ...slot, episode: 2 } : slot),
  });
  await expect(page.locator('.bot-card[data-bot-id="bot-4"] .bot-facts')).toContainText("Game 2");
  await expectBotWindow(["bot-3", "bot-4", "bot-5"], "Bots 3-5 of 10");
  for (let first = 4; first <= 8; first += 1) {
    await page.locator("#bots-next").click();
    await expectBotWindow([`bot-${first}`, `bot-${first + 1}`, `bot-${first + 2}`],
      `Bots ${first}-${first + 2} of 10`);
  }
  await expect(page.locator("#bots-next")).toBeDisabled();
  await expect(page.locator("#bots-previous")).toBeEnabled();
  await expect(page.locator("#session-select")).toHaveValue("bot-10");
  await expect(page.locator(".decision-result span")).toHaveText("1000");
  await expect(page.locator("#population-results")).toContainText("58.0%");
  await expect(page.locator("#bot-count")).toHaveText("10 running");
  expect(commands).toHaveLength(commandsBeforeBrowsing);
  await page.locator('[data-bot-id="bot-10"][data-bot-action="stop"]').click();
  await expect.poll(() => commands.includes("/api/bots/bot-10/stop")).toBe(true);
  for (let first = 7; first >= 1; first -= 1) {
    await page.locator("#bots-previous").click();
    await expectBotWindow([`bot-${first}`, `bot-${first + 1}`, `bot-${first + 2}`],
      `Bots ${first}-${first + 2} of 10`);
  }
  await expect(page.locator("#bots-previous")).toBeDisabled();
  expect((await page.locator("#board").boundingBox()).height).toBe(humanBox.height);
  await page.locator("#board").click();
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => commands.includes("/api/commands/move-right")).toBe(true);
  await page.locator("#bots-toggle").click();
  await expect(page.locator("#bot-grid")).toBeHidden();
  await expect(page.locator("#bots-pagination")).toBeHidden();
  const collapsedBox = await page.locator("#board").boundingBox();
  expect(collapsedBox.height).toBe(humanBox.height);
  await expect(page.locator("#bot-count")).toHaveText("10 running");
  await page.locator("#bots-toggle").click();
  await expect(page.locator("#bots-pagination")).toBeVisible();
  await expectBotWindow(["bot-1", "bot-2", "bot-3"], "Bots 1-3 of 10");
  await page.locator("#bots-stop").click();
  await expect.poll(() => commands.includes("/api/bots/stop")).toBe(true);
});

function publishState(value) {
  const view = value.analysisActions;
  const observations = view.observations.length > 0 ? view.observations
    : view.actions.flatMap((action) => {
      const base = {
        contractDigest: action.contractDigest, contractName: action.contractName,
        cycleId: action.cycleId, attemptId: action.attemptId, actionId: action.actionId,
        kind: action.kind === "unknown/conflicting" ? "artifact.write" : action.kind,
        attributes: action.attributes, traceId: action.traceId,
      };
      return [
        ...(action.startedAt === undefined ? [] : [{
          ...base, event: "started", observedAt: action.startedAt, outcome: "running",
        }]),
        ...(action.outcome === undefined ? [] : [{
          ...base, event: "completed", observedAt: action.completedAt,
          outcome: action.outcome,
        }]),
        ...(action.status !== "unknown/conflicting" ? [] : ["success", "failed"].map((outcome) => ({
          ...base, event: "completed", observedAt: action.latestObservedAt, outcome,
        }))),
      ];
    });
  value = { ...value, analysisActions: {
    ...createAnalysisActionsView({
      logs: [], observations, now: new Date(value.observedAt), queryLimited: view.limited,
      ...(view.error === undefined ? {} : { error: view.error }),
    }),
    status: view.status,
  } };
  inspectionSnapshot = value;
  for (const client of eventClients) {
    client.write(`event: state\ndata: ${JSON.stringify(value)}\n\n`);
  }
}

async function sendFile(response, relativePath, contentType) {
  response.writeHead(200, {
    "Content-Type": `${contentType}; charset=utf-8`,
  });
  response.end(await readFile(resolve(exampleDirectory, relativePath)));
}
