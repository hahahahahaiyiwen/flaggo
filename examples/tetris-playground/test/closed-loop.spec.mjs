import { access, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { expect, test } from "@playwright/test";
import { createDecisionClient } from "@flaggo/sdk/runtime";
import { parseListeningUrl } from "../../tetris-integration/host-process.mjs";
import { parseLokiResponse, parseTempoResponse } from "../dist/backend.js";
import { startPlaygroundFixture } from "./playground-fixture.mjs";

const exampleDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);
const analysisMode = process.env.FLAGGO_PLAYGROUND_E2E_ANALYSIS_MODE
  ?? "deterministic";
if (analysisMode !== "deterministic" && analysisMode !== "copilot") {
  throw new Error(
    "FLAGGO_PLAYGROUND_E2E_ANALYSIS_MODE must be deterministic or copilot.",
  );
}
const demoTimeout = analysisMode === "copilot" ? 660_000 : 90_000;
let host;
let playgroundUrl;

test.beforeAll(async () => {
  test.setTimeout(660_000);
  host = await startPlaygroundFixture({ mode: "benchmark", analysisMode, retain: true });
  playgroundUrl = host.url;
});

test.afterAll(async () => {
  await host?.close();
  if (host !== undefined) {
    await rm(host.runDirectory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    await expect(access(host.runDirectory)).rejects.toThrow();
  }
});

test.afterEach(async ({}, testInfo) => {
  if (testInfo.status === testInfo.expectedStatus || host === undefined) return;
  const log = await readFile(resolve(host.runDirectory, "async-analysis.log"), "utf8");
  const path = testInfo.outputPath("async-analysis.log");
  await writeFile(path, log.slice(-65_536), "utf8");
  await testInfo.attach("analysis-diagnostics", { path });
});

test("learns adaptive gravity and improves matched held-out gameplay", async ({
  page,
}, testInfo) => {
  test.setTimeout(demoTimeout + 30_000);
  await page.goto(playgroundUrl);
  await expect(page.locator("#connection")).toHaveText("Live");
  await expect(page.locator("#board .cell")).toHaveCount(200);

  await page.locator('[data-command="hard-drop"]').click();
  await expect(page.locator("#score")).not.toHaveText("0");

  await page.locator("#demo-run").click();
  await expect(
    page.locator(".demo-summary.completed, .demo-summary.failed"),
  ).toBeVisible({
    timeout: demoTimeout,
  });
  const terminalStateResponse = await page.request.get(
    `${playgroundUrl}/api/state`,
  );
  expect(terminalStateResponse.ok()).toBe(true);
  const terminalState = await terminalStateResponse.json();
  const evaluationPath = testInfo.outputPath("adaptive-evaluation.json");
  await writeFile(evaluationPath, JSON.stringify(terminalState, null, 2), "utf8");
  await testInfo.attach("terminal-evaluation", {
    path: evaluationPath,
  });
  expect(terminalState.demo.stage, terminalState.demo.detail).toBe("completed");
  const { baseline, learned, balanceImprovement } = terminalState.demo;
  for (const result of [baseline, learned]) {
    expect(result.episodes).toBe(3);
    expect(result.plannedPlayMs).toBe(360_000);
    expect(result.activePlayMs).toBeGreaterThan(0);
    expect(result.activePlayMs).toBeLessThanOrEqual(result.plannedPlayMs);
    expect(result.challengeBalance).toBeGreaterThanOrEqual(0);
    expect(result.challengeBalance).toBeLessThanOrEqual(1);
    expect(result.linesCleared).toBeGreaterThan(0);
    expect(result.linesPerMinute).toBeCloseTo(
      result.linesCleared * 60_000 / result.activePlayMs,
    );
    expect(result.gameOvers).toBeGreaterThanOrEqual(0);
    expect(result.gameOvers).toBeLessThanOrEqual(result.episodes);
    expect(result.executableDigest).toMatch(/^sha256:[0-9a-f]{64}$/u);
  }
  expect(learned.executableDigest).not.toBe(baseline.executableDigest);
  expect(balanceImprovement).toBeCloseTo(
    learned.challengeBalance - baseline.challengeBalance,
  );
  expect(balanceImprovement + Number.EPSILON).toBeGreaterThanOrEqual(0.05);
  expect(learned.linesPerMinute).toBeGreaterThanOrEqual(
    baseline.linesPerMinute * 0.9,
  );
  expect(learned.gameOvers).toBeLessThanOrEqual(baseline.gameOvers);
  expect(learned.activePlayMs).toBeGreaterThanOrEqual(baseline.activePlayMs);
  await expect(page.locator(".comparison")).toContainText("balanced");
  await expect(page.locator(".comparison")).toContainText("lines/min");
  await expect(page.locator(".comparison")).toContainText("survived");
  await expect(page.locator(".health-pill.failed")).toHaveCount(0);
  await expect(page.locator(".health-pill.stopped")).toHaveCount(0);
  await expect(page.locator(".phase-activation").first()).toBeVisible();
  let state;
  await expect.poll(async () => {
    const response = await page.request.get(`${playgroundUrl}/api/state`);
    expect(response.ok()).toBe(true);
    state = await response.json();
    const cycle = state.executables.activeExecutable.provenance?.cycleId;
    return state.timeline.some((event) =>
      event.name === "flaggo.analysis.cycle.completed" && event.outcome === "candidate"
      && event.attributes["flaggo.analysis.cycle.id"] === cycle)
      && state.controlLoop.candidate?.candidateDigest === learned.executableDigest
      && state.controlLoop.activation?.executableDigest === learned.executableDigest;
  }, { timeout: 30_000 }).toBe(true);
  await writeFile(evaluationPath, JSON.stringify(state, null, 2), "utf8");
  expect(state.backendFailures).toEqual([]);
  expect(state.metrics.length).toBeGreaterThan(0);
  expect(state.evidence.observationsCreated).toBeGreaterThan(0);
  expect(state.services.every((service) => service.status === "healthy")).toBe(
    true,
  );
  expect(state.executables.status).toBe("ready");
  expect(state.executables.activeExecutable.executableDigest).toBe(
    learned.executableDigest,
  );
  expect(state.controlLoop.candidate.status).toBe("success");
  expect(state.controlLoop.activation.status).toBe("success");
  for (const phase of [
    "decision",
    "materialization",
    "analysis",
    "candidate",
    "activation",
  ]) {
    expect(state.timeline.some((event) => event.phase === phase)).toBe(true);
  }
  const active = state.executables.activeExecutable;
  const provenance = active.provenance;
  expect(active.contractDigest).toBe(state.executables.contractDigest);
  expect(provenance.source).toBe("async-analysis");
  expect(provenance.cycleId).toMatch(/^[0-9a-f-]{36}$/u);
  expect(provenance.attemptId).toMatch(/^[0-9a-f-]{36}$/u);
  expect(provenance.evidenceWatermark).toBeGreaterThan(0);
  expect(Date.parse(provenance.evidenceCutoff)).toBeLessThanOrEqual(
    Date.parse(active.createdAt),
  );
  expect(provenance.analysisManifestDigest).toMatch(/^sha256:[0-9a-f]{64}$/u);
  expect(state.controlLoop.candidate.candidateDigest).toBe(learned.executableDigest);
  expect(state.controlLoop.activation.executableDigest).toBe(learned.executableDigest);
  for (const service of state.services) {
    expect(state.metrics.some((metric) => metric.service === service.name),
      `${service.name} must export operational metrics`).toBe(true);
  }
  const urls = Object.fromEntries(await Promise.all([
    ["contractServiceUrl", "contract-service.log"],
    ["decisionServiceUrl", "decision-service.log"],
    ["otlpIngestionUrl", "otel-ingestion.log"],
  ].map(async ([name, file]) => {
    const log = await readFile(resolve(host.runDirectory, file), "utf8");
    const url = log.split(/\r?\n/u).map(parseListeningUrl).find((value) => value !== undefined);
    expect(url, `${file} must report its bound loopback address`).toBeDefined();
    return [name, url];
  })));
  const manifest = JSON.parse(await readFile(resolve(exampleDirectory, "flaggo.deploy.json"), "utf8"));
  const runtimeConfig = {
    format: "flaggo.runtime-config/v1",
    authority: manifest.authority,
    services: urls,
    bindings: { "tetris.dropInterval": { contractDigest: active.contractDigest } },
  };
  const client = createDecisionClient({ runtimeConfig, random: () => 0.25 });
  const decisions = [];
  for (const pressure of [0.1, 0.3]) {
    const attributes = {
      board_pressure_mean_5s: pressure, board_pressure_max_5s: pressure,
      hard_drop_ratio_5s: 0.5, placement_time_mean_ms_5s: 3_000,
      pieces_locked_5s: 2, active_play_ms_5s: 5_000,
      challenge_balance_5s: 0.5, lines_cleared_5s: 0, game_over_count_5s: 0,
      current_drop_interval_ms: 800, session_id: "phase4-exit-parity",
      exposure_id: `unused-${pressure}`,
    };
    const sdk = (await client.decide("tetris.dropInterval", { attributes })).value;
    const restResponse = await page.request.post(
      `${urls.decisionServiceUrl}/v3/decision-contracts/tetris.dropInterval/versions/${active.contractDigest}/decisions`,
      { data: { attributes: { ...attributes, _random: 0.25 } } },
    );
    expect(restResponse.ok()).toBe(true);
    const rest = await restResponse.json();
    for (const decision of [sdk, rest]) {
      expect(decision.contractDigest).toBe(active.contractDigest);
      expect(decision.executableDigest).toBe(learned.executableDigest);
    }
    expect(rest.result).toBe(sdk.result);
    expect(rest.evaluation).toEqual(sdk.evaluation);
    if (analysisMode === "deterministic") {
      expect(sdk.result).toBe(pressure < 0.15 ? 200 : 1_000);
    }
    decisions.push({ attributes, sdk, rest });
  }
  const conflict = await page.request.post(
    `${urls.contractServiceUrl}/v3/decision-contracts/tetris.dropInterval/versions/${active.contractDigest}/executables/${baseline.executableDigest}/activate`,
    { data: {
      expectedActiveExecutableDigest: baseline.executableDigest,
      expectedActiveStateVersion: 0,
      expectedCandidateStateVersion: 0,
    } },
  );
  expect(conflict.status()).toBe(409);
  const catalog = await page.request.get(
    `${urls.contractServiceUrl}/v3/decision-contracts/tetris.dropInterval/versions/${active.contractDigest}/executables`,
  );
  expect(catalog.ok()).toBe(true);
  expect((await catalog.json()).activeExecutable.executableDigest).toBe(learned.executableDigest);
  const rejectedExport = await page.request.post(`${urls.otlpIngestionUrl}/v1/logs`, {
    headers: { "Content-Type": "application/xml" }, data: "<invalid/>",
  });
  expect(rejectedExport.status()).toBe(415);
  const execute = promisify(execFile);
  const containers = await execute("docker", [
    "ps", "--filter", `name=flaggo-playground-${host.child.pid}-`,
    "--filter", "label=com.docker.compose.service=lgtm", "--format", "{{.ID}}",
  ]);
  const containerIds = containers.stdout.trim().split(/\r?\n/u).filter(Boolean);
  expect(containerIds).toHaveLength(1);
  const backendPorts = await execute("docker", [
    "inspect", "--format", "{{json .NetworkSettings.Ports}}", containerIds[0],
  ]);
  const ports = JSON.parse(backendPorts.stdout);
  const endpoint = (port) => {
    expect(ports[port]).toHaveLength(1);
    expect(ports[port][0].HostIp).toBe("127.0.0.1");
    return `http://127.0.0.1:${ports[port][0].HostPort}`;
  };
  const tempoUrl = endpoint("3200/tcp");
  const lokiUrl = endpoint("3100/tcp");
  const actionProfile = JSON.parse(await readFile(
    resolve(exampleDirectory, "..", "..", "contracts", "otel",
      "flaggo-service-observability-profile-v1.json"), "utf8",
  ));
  let actionLogs;
  const actionPath = testInfo.outputPath("analysis-actions.json");
  await expect.poll(async () => {
    const response = await page.request.get(`${lokiUrl}/loki/api/v1/query_range?${new URLSearchParams({
      query: `{service_name="flaggo-async-analysis"} | flaggo_analysis_cycle_id = "${provenance.cycleId}" | event_name =~ "flaggo.analysis.action.(started|completed)"`,
      start: String(BigInt(Date.parse(state.demo.startedAt) - 60_000) * 1_000_000n),
      limit: "500",
      direction: "forward",
    })}`);
    expect(response.ok(), await response.text()).toBe(true);
    actionLogs = parseLokiResponse(await response.json());
    const completed = actionLogs.filter((log) =>
      log.eventName === "flaggo.analysis.action.completed");
    return ["session.start", "session.run", "evidence.describe", "evidence.cutoff",
      "evidence.sql", "artifact.write", "proposal.submit"].every((kind) =>
      completed.some((log) => log.attributes["flaggo.analysis.action.kind"] === kind));
  }, { timeout: 60_000, message: "Candidate cycle must export granular action observations" }).toBe(true)
    .finally(async () => {
      await writeFile(actionPath, JSON.stringify(actionLogs ?? [], null, 2), "utf8");
      await testInfo.attach("analysis-actions", { path: actionPath });
    });
  const actionDefinitions = new Map(actionProfile.logEvents
    .filter((event) => event.eventName.startsWith("flaggo.analysis.action."))
    .map((event) => [event.eventName, event]));
  const attributeDefinitions = new Map(actionProfile.attributes.map((field) => [field.name, field]));
  for (const log of actionLogs) {
    const definition = actionDefinitions.get(log.eventName);
    expect(definition).toBeDefined();
    const allowed = new Set([
      ...definition.requiredAttributes, ...definition.conditionalAttributes.map((field) => field.name),
    ]);
    for (const name of definition.requiredAttributes) {
      expect(log.attributes[name], `Missing ${name}`).toBeDefined();
    }
    for (const [name, value] of Object.entries(log.attributes)) {
      if (!name.startsWith("flaggo.")) continue;
      expect(allowed.has(name), `Unexpected action attribute ${name}`).toBe(true);
      const field = attributeDefinitions.get(name);
      if (field.allowedValues !== undefined) expect(field.allowedValues).toContain(value);
      if (field.type === "int64") {
        expect(["string", "number"]).toContain(typeof value);
        if (typeof value === "string") expect(value).toMatch(/^(0|[1-9]\d*)$/u);
        const measured = Number(value);
        expect(Number.isSafeInteger(measured), `${name} must encode an integer`).toBe(true);
        expect(measured).toBeGreaterThanOrEqual(0);
      } else {
        expect(typeof value).toBe("string");
        expect(Buffer.byteLength(value, "utf8")).toBeLessThanOrEqual(256);
      }
      expect(field.metricDimension).toBe(false);
    }
    expect(log.attributes["flaggo.contract.digest"]).toBe(active.contractDigest);
    expect(log.attributes["flaggo.analysis.cycle.id"]).toBe(provenance.cycleId);
    expect(log.attributes["flaggo.analysis.attempt.id"]).toBe(provenance.attemptId);
    expect(log.attributes["flaggo.analysis.action.id"]).toMatch(/^[0-9a-f-]{36}$/u);
    expect(log.traceId).toMatch(/^[0-9a-f]{32}$/u);
  }
  const sqlActions = actionLogs.filter((log) =>
    log.eventName === "flaggo.analysis.action.completed"
    && log.attributes["flaggo.analysis.action.kind"] === "evidence.sql"
    && log.attributes["flaggo.analysis.action.outcome"] === "success");
  expect(sqlActions.length).toBeGreaterThan(0);
  for (const log of sqlActions) {
    expect(log.attributes["flaggo.analysis.query.digest"]).toMatch(/^sha256:[0-9a-f]{64}$/u);
    expect(Date.parse(log.attributes["flaggo.analysis.snapshot.cutoff"]))
      .toBe(Date.parse(provenance.evidenceCutoff));
    expect(Number(log.attributes["flaggo.analysis.snapshot.watermark"])).toBe(provenance.evidenceWatermark);
    expect(Number(log.attributes["flaggo.analysis.result.rows"])).toBeGreaterThan(0);
    expect(Number(log.attributes["flaggo.analysis.result.bytes"])).toBeGreaterThan(0);
  }
  const cycleSelection = JSON.stringify([active.contractDigest, provenance.cycleId]);
  await expect.poll(() => page.locator("#analysis-cycle option")
    .evaluateAll((options) => options.map((option) => option.value)),
  { timeout: 30_000 }).toContain(cycleSelection);
  await page.locator("#analysis-cycle").selectOption(cycleSelection);
  await page.locator("#analysis-attempt").selectOption(provenance.attemptId);
  const inspectedSqlId = sqlActions[0].attributes["flaggo.analysis.action.id"];
  const inspectedSql = page.locator(`.analysis-action[data-action-id="${inspectedSqlId}"]`);
  await expect(inspectedSql).toBeVisible({ timeout: 30_000 });
  await inspectedSql.locator("summary").click();
  await expect(inspectedSql).toHaveAttribute("open", "");
  for (const label of ["Duration (ms)", "Rows", "Bytes", "Query digest", "Cutoff", "Watermark"]) {
    await expect(inspectedSql).toContainText(label);
  }
  await expect(inspectedSql.locator(".action-trace")).toBeVisible();
  await expect.poll(async () => {
    const response = await page.request.get(`${playgroundUrl}/api/state`);
    state = await response.json();
    return state.analysisActions.actions.some((action) =>
      action.actionId === inspectedSqlId && action.cycleId === provenance.cycleId
      && action.attemptId === provenance.attemptId && action.outcome === "success");
  }, { timeout: 30_000 }).toBe(true);
  const telemetry = {};
  await expect.poll(async () => {
    await Promise.all(state.services.map(async ({ name }) => {
      const [logsResponse, tracesResponse] = await Promise.all([
        page.request.get(`${lokiUrl}/loki/api/v1/query_range?${new URLSearchParams({
          query: `{service_name="${name}"} | trace_id != ""`,
          start: String(BigInt(Date.parse(state.demo.startedAt) - 60_000) * 1_000_000n),
          limit: "10",
        })}`),
        page.request.get(`${tempoUrl}/api/search?${new URLSearchParams({
          q: `{ resource.service.name = "${name}" && name =~ "^flaggo\\\\..*" }`,
          start: String(Math.floor(Date.parse(state.demo.startedAt) / 1_000) - 60),
          end: String(Math.ceil(Date.now() / 1_000)),
          limit: "60",
        })}`),
      ]);
      expect(logsResponse.ok(), await logsResponse.text()).toBe(true);
      expect(tracesResponse.ok(), await tracesResponse.text()).toBe(true);
      telemetry[name] = {
        logs: parseLokiResponse(await logsResponse.json()),
        spans: parseTempoResponse(await tracesResponse.json()),
      };
      const log = telemetry[name].logs.find((value) => value.traceId !== undefined);
      if (log !== undefined) {
        const traceResponse = await page.request.get(`${tempoUrl}/api/traces/${log.traceId}`);
        if (traceResponse.status() !== 404) {
          expect(traceResponse.ok(), await traceResponse.text()).toBe(true);
          telemetry[name].correlatedTrace = { traceId: log.traceId, trace: await traceResponse.json() };
        }
      }
    }));
    return Object.values(telemetry).every(({ logs, spans, correlatedTrace }) =>
      logs.length > 0 && spans.length > 0
      && correlatedTrace !== undefined);
  }, { timeout: 60_000, message: "Every service must have correlated logs and traces in LGTM" }).toBe(true)
    .finally(async () => {
      const path = testInfo.outputPath("service-telemetry.json");
      await writeFile(path, JSON.stringify(telemetry, null, 2), "utf8");
      await testInfo.attach("service-telemetry", { path });
    });
  await writeFile(evaluationPath, JSON.stringify({
    ...state, exitVerification: {
      runtimeConfig, decisions, telemetry, actionLogs,
      rejectedActivation: await conflict.json(), rejectedExport: await rejectedExport.json(),
    },
  }, null, 2), "utf8");
  await testInfo.attach("phase4-exit-verification", { path: evaluationPath });
  await host.close();
  await expect(fetch(`${playgroundUrl}/api/health`, {
    signal: AbortSignal.timeout(2_000),
  })).rejects.toThrow();
});
