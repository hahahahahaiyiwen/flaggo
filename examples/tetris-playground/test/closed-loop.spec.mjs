import { readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";
import { startPlaygroundFixture } from "./playground-fixture.mjs";

const exampleDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);
const repositoryRoot = resolve(exampleDirectory, "../..");
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
  host = await startPlaygroundFixture({ mode: "benchmark", analysisMode });
  playgroundUrl = host.url;
});

test.afterAll(async () => {
  await host?.close();
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
  await testInfo.attach("adaptive-evaluation", {
    path: evaluationPath,
  });
  if (terminalState.demo.stage !== "completed") {
    const directory = (await readdir(resolve(repositoryRoot, ".flaggo")))
      .find((name) => name.startsWith(`tetris-playground-${host.child.pid}-`));
    if (directory === undefined) {
      throw new Error("The failed playground's diagnostic directory is missing.");
    }
    const log = await readFile(resolve(
      repositoryRoot, ".flaggo", directory, "async-analysis.log",
    ), "utf8");
    const logPath = testInfo.outputPath("async-analysis.log");
    await writeFile(logPath, log.slice(-65_536), "utf8");
    await testInfo.attach("analysis-diagnostics", { path: logPath });
  }
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
  const stateResponse = await page.request.get(`${playgroundUrl}/api/state`);
  expect(stateResponse.ok()).toBe(true);
  const state = await stateResponse.json();
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
  expect(state.controlLoop.analysis.status).toBe("success");
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
});
