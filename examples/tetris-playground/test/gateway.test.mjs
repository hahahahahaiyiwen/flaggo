import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const exampleDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);
const repositoryRoot = resolve(exampleDirectory, "../..");
const liveContract = JSON.parse(await readFile(resolve(
  exampleDirectory, "flaggo/contracts/live/tetris.dropInterval.decision-contract.json",
), "utf8"));
const contractDigest = `sha256:${"0".repeat(64)}`;
const executableDigest = `sha256:${"1".repeat(64)}`;
const candidateDigest = `sha256:${"2".repeat(64)}`;
const inactiveDigest = `sha256:${"3".repeat(64)}`;

test("gateway serves state, commands, SSE, decisions, and graceful shutdown", {
  timeout: 30_000,
}, async (context) => {
  const traceparents = [];
  const decisionInputs = [];
  let activeExecutableDigest = executableDigest;
  let candidateAvailable = true;
  let activationPayload;
  let emitActivity = true;
  let failLogSample = false;
  let managementDelayMs = 0;
  let managementRequests = 0;
  const actionQueries = [];
  let floodActions = false;
  const actionId = "00000000-0000-4000-8000-000000000001";
  const activityAt = String(BigInt(Date.now()) * 1_000_000n);
  const backend = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (request.method === "GET" && /\/executables(?:\/history)?$/u.test(url.pathname)) {
      managementRequests += 1;
      if (managementDelayMs > 0) {
        await new Promise((resolve) => setTimeout(resolve, managementDelayMs));
      }
    }
    if (url.pathname.endsWith("/decisions")) {
      const input = JSON.parse(await readBody(request)).attributes;
      decisionInputs.push(input);
      traceparents.push(request.headers.traceparent);
      const noData = input.active_play_ms_5s === 0 || input.pieces_locked_5s === 0;
      const learned = activeExecutableDigest === candidateDigest;
      const interval = learned
        ? input.board_pressure_mean_5s < 0.15 ? 200 : 1_000
        : [200, 400, 600, 800, 1_000][Math.floor(input._random * 5)];
      return json(response, 200, {
        contractDigest,
        executableDigest: activeExecutableDigest,
        result: noData ? input.current_drop_interval_ms : interval,
        evaluation: {
          source: "rule",
          rule: noData
            ? "retain-current-without-observations"
            : learned ? "learned-context" : "explore-gravity",
        },
      });
    }
    if (
      request.method === "GET"
      && url.pathname.endsWith("/executables")
    ) {
      return json(response, 200, {
        contractName: "tetris.dropInterval",
        contractDigest,
        activeExecutable: managedExecutable(
          activeExecutableDigest,
          "active",
          activeExecutableDigest === candidateDigest ? 2 : 1,
        ),
        candidates: candidateAvailable
          ? [managedExecutable(candidateDigest, "candidate", 0)]
          : [],
        nextCursor: null,
      });
    }
    if (
      request.method === "GET"
      && url.pathname.endsWith("/executables/history")
    ) {
      const older = url.searchParams.get("cursor") === "older";
      return json(response, 200, {
        contractName: "tetris.dropInterval",
        contractDigest,
        executables: older
          ? [managedExecutable(inactiveDigest, "inactive", 2)]
          : [
            managedExecutable(candidateDigest, candidateAvailable ? "candidate" : "active",
              candidateAvailable ? 0 : 2),
            managedExecutable(executableDigest, candidateAvailable ? "active" : "inactive",
              candidateAvailable ? 1 : 2),
          ],
        nextCursor: older ? null : "older",
      });
    }
    if (
      request.method === "POST"
      && url.pathname.endsWith("/activate")
    ) {
      activationPayload = JSON.parse(await readBody(request));
      const previousExecutableDigest = activeExecutableDigest;
      activeExecutableDigest = candidateDigest;
      candidateAvailable = false;
      return json(response, 200, {
        contractName: "tetris.dropInterval",
        contractDigest,
        executable: managedExecutable(candidateDigest, "active", 2),
        previousExecutableDigest,
        supersededCandidateCount: 0,
      });
    }
    if (url.pathname.startsWith("/v1/")) {
      await readBody(request);
      response.writeHead(200, {
        "Content-Type": "application/x-protobuf",
      });
      response.end();
      return;
    }
    if (url.pathname === "/health/ready") {
      return json(response, 200, {
        status: "ready",
        inbox: { retainedBatchCount: 0 },
      });
    }
    if (url.pathname === "/loki/api/v1/query_range") {
      if (failLogSample && url.searchParams.get("direction") === "forward") {
        return json(response, 503, { error: "Log sample unavailable." });
      }
      const scoped = url.searchParams.get("query").includes("flaggo_analysis_cycle_id");
      if (scoped) {
        actionQueries.push(url.searchParams.get("query"));
      }
      const actionMetadata = {
        otel_scope_name: "flaggo.async-analysis",
        flaggo_contract_name: "tetris.dropInterval", flaggo_contract_digest: contractDigest,
        flaggo_analysis_cycle_id: "cycle-1", flaggo_analysis_attempt_id: "attempt-1",
        flaggo_analysis_action_id: actionId, flaggo_analysis_action_kind: "evidence.sql",
        flaggo_analysis_query_digest: `sha256:${"4".repeat(64)}`,
      };
      return json(response, 200, {
        status: "success",
        data: { resultType: "streams", result: emitActivity ? [{
          stream: { service_name: "flaggo-async-analysis" },
          values: [
            ...(floodActions && !scoped ? Array.from({ length: 400 }, (_, index) => [
              String(BigInt(activityAt) + BigInt(index + 1) * 1_000_000n), "Newer action", {
                ...actionMetadata,
                event_name: "flaggo.analysis.action.completed",
                flaggo_analysis_action_id: `10000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
                flaggo_analysis_cycle_id: "newer-cycle",
                flaggo_analysis_attempt_id: "newer-attempt",
                flaggo_analysis_action_outcome: "success",
                flaggo_analysis_action_duration_ms: "1",
              },
            ]) : []),
            [activityAt, "Completed analysis", {
            event_name: "flaggo.analysis.cycle.completed",
            otel_scope_name: "flaggo.async-analysis",
            flaggo_operation_outcome: "no_candidate",
          }], [activityAt, "PRIVATE prompt and SQL", {
            ...actionMetadata, event_name: "flaggo.analysis.action.started",
            flaggo_analysis_action_outcome: "running", private_prompt: "PRIVATE",
          }], [activityAt, "PRIVATE dataset and provider logs", {
            ...actionMetadata, event_name: "flaggo.analysis.action.completed",
            flaggo_analysis_action_outcome: "success", flaggo_analysis_action_duration_ms: "5",
            flaggo_analysis_result_rows: "2", private_dataset: "PRIVATE",
          }]],
        }] : [] },
      });
    }
    if (url.pathname === "/api/search") {
      return json(response, 200, { traces: [] });
    }
    if (url.pathname === "/api/v1/query") {
      return json(response, 200, {
        status: "success",
        data: { resultType: "vector", result: [] },
      });
    }
    response.writeHead(404).end();
  });
  backend.listen(0, "127.0.0.1");
  await once(backend, "listening");
  context.after(async () => {
    await new Promise((resolvePromise, reject) => {
      backend.close((error) => {
        if (error === undefined) resolvePromise();
        else reject(error);
      });
    });
  });
  const backendUrl = serverUrl(backend);
  const gatewayPort = await reservePort();
  const gatewayUrl = `http://127.0.0.1:${gatewayPort}`;
  let output = "";
  const child = spawn(
    process.execPath,
    [resolve(exampleDirectory, "dist/server.js")],
    {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        FLAGGO_PLAYGROUND_ADDRESS: `127.0.0.1:${gatewayPort}`,
        FLAGGO_PLAYGROUND_MODE: "live",
        FLAGGO_PLAYGROUND_GRAFANA_URL: backendUrl,
        FLAGGO_PLAYGROUND_HEALTH_URLS: JSON.stringify({
          "flaggo-contract-service": `${backendUrl}/health/ready`,
          "flaggo-decision-service": `${backendUrl}/health/ready`,
          "flaggo-otel-ingestion": `${backendUrl}/health/ready`,
        }),
        FLAGGO_PLAYGROUND_LOKI_URL: backendUrl,
        FLAGGO_PLAYGROUND_PROMETHEUS_URL: backendUrl,
        FLAGGO_PLAYGROUND_RUNTIME_CONFIG: JSON.stringify({
          format: "flaggo.runtime-config/v1",
          authority: {
            tenant: "local",
            application: "tetris",
            environment: "gateway-test",
          },
          services: {
            contractServiceUrl: backendUrl,
            decisionServiceUrl: backendUrl,
            otlpIngestionUrl: backendUrl,
          },
          bindings: {
            "tetris.dropInterval": { contractDigest },
          },
        }),
        FLAGGO_PLAYGROUND_SERVICE_PROCESSES: JSON.stringify({
          "flaggo-contract-service": process.pid,
          "flaggo-decision-service": process.pid,
          "flaggo-otel-ingestion": process.pid,
          "flaggo-evidence-materializer": process.pid,
          "flaggo-async-analysis": process.pid,
        }),
        FLAGGO_PLAYGROUND_TEMPO_URL: backendUrl,
      },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  child.stdout.on("data", (chunk) => {
    output = `${output}${String(chunk)}`.slice(-16_384);
  });
  child.stderr.on("data", (chunk) => {
    output = `${output}${String(chunk)}`.slice(-16_384);
  });
  context.after(() => {
    if (child.exitCode === null) child.kill();
  });

  await waitFor(async () => {
    const response = await fetch(`${gatewayUrl}/api/health`);
    return response.ok;
  }, 10_000, () => output);

  const state = await fetchJson(`${gatewayUrl}/api/state`);
  assert.equal(state.human.state.board.length, 20);
  assert.equal(state.mode, "live");
  assert.equal(state.bots.length, 10);
  assert.ok(state.bots.every((slot) => slot.status === "idle"));
  assert.equal(state.services.length, 5);
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.executables.status === "ready"
      && current.executables.candidates.length === 1;
  }, 5_000, () => output);

  const olderResponse = await fetch(`${gatewayUrl}/api/executables/history?cursor=older`, {
    method: "POST",
  });
  assert.equal(olderResponse.status, 200);
  const olderState = await olderResponse.json();
  assert.equal(olderState.executables.historyCursor, "older");
  assert.equal(olderState.executables.history[0].executableDigest, inactiveDigest);
  assert.equal(olderState.executables.history[0].lifecycleState, "inactive");
  assert.equal(olderState.executables.activeExecutable.executableDigest, executableDigest);
  const newestResponse = await fetch(`${gatewayUrl}/api/executables/history`, { method: "POST" });
  assert.equal(newestResponse.status, 200);
  const newestState = await newestResponse.json();
  assert.equal(newestState.executables.historyCursor, undefined);
  assert.equal(newestState.executables.history.length, 2);
  assert.equal(newestState.executables.nextCursor, "older");
  managementDelayMs = 2_000;
  const slowHistoryResponse = await fetch(`${gatewayUrl}/api/executables/history`, { method: "POST" });
  assert.equal(slowHistoryResponse.status, 200);
  const slowHistoryState = await slowHistoryResponse.json();
  assert.equal(slowHistoryState.executables.status, "ready");
  managementDelayMs = 6_000;
  const requestCount = managementRequests;
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return managementRequests > requestCount && current.executables.status === "unavailable";
  }, 9_000, () => output);
  const timedOutState = await fetchJson(`${gatewayUrl}/api/state`);
  assert.match(timedOutState.executables.error, /timeout/u);
  assert.equal(timedOutState.executables.activeExecutable.executableDigest, executableDigest);
  assert.deepEqual(timedOutState.executables.history, slowHistoryState.executables.history);
  managementDelayMs = 0;
  await waitFor(async () => (await fetchJson(`${gatewayUrl}/api/state`)).executables.status === "ready",
    8_000, () => output);

  const events = await fetch(`${gatewayUrl}/api/events`);
  assert.equal(events.status, 200);
  const reader = events.body.getReader();
  const firstEvent = await reader.read();
  assert.match(
    new TextDecoder().decode(firstEvent.value),
    /event: state\ndata: /u,
  );
  await reader.cancel();

  const command = await fetch(`${gatewayUrl}/api/commands/move-left`, {
    method: "POST",
  });
  assert.equal(command.status, 200);
  const invalid = await fetch(`${gatewayUrl}/api/commands/not-a-command`, {
    method: "POST",
  });
  assert.equal(invalid.status, 400);

  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.human.decision?.source === "flaggo"
      && current.human.decision.result === 800;
  }, 5_000, () => output);
  assert.deepEqual(Object.keys(decisionInputs[0]).sort(), [
    "_random",
    "active_play_ms_5s",
    "board_pressure_max_5s",
    "board_pressure_mean_5s",
    "challenge_balance_5s",
    "current_drop_interval_ms",
    "exposure_id",
    "game_over_count_5s",
    "hard_drop_ratio_5s",
    "lines_cleared_5s",
    "pieces_locked_5s",
    "placement_time_mean_ms_5s",
    "session_id",
  ].sort());
  assert.equal(decisionInputs[0].pieces_locked_5s, 0);
  assert.equal(decisionInputs[0].current_drop_interval_ms, 800);
  assert.equal(decisionInputs[0].session_id, state.human.state.sessionId);
  assert.ok(decisionInputs[0].exposure_id.length > 0);
  assert.ok(
    traceparents.some((value) =>
      /^00-[0-9a-f]{32}-[0-9a-f]{16}-[0-9a-f]{2}$/u.test(value ?? "")
    ),
    "Decision request must carry W3C traceparent.",
  );

  for (const action of ["hard-drop", "move-left"]) {
    const response = await fetch(`${gatewayUrl}/api/commands/${action}`, {
      method: "POST",
    });
    assert.equal(response.status, 200);
  }
  const activation = await fetch(
    `${gatewayUrl}/api/executables/${encodeURIComponent(candidateDigest)}/activate`,
    { method: "POST" },
  );
  assert.equal(activation.status, 200);
  assert.deepEqual(activationPayload, {
    expectedActiveExecutableDigest: executableDigest,
    expectedActiveStateVersion: 1,
    expectedCandidateStateVersion: 0,
  });
  const activatedState = await activation.json();
  assert.equal(
    activatedState.executables.activeExecutable.executableDigest,
    candidateDigest,
  );
  assert.equal(activatedState.executables.candidates.length, 0);
  assert.equal(activatedState.executables.history.find((value) =>
    value.executableDigest === executableDigest
  ).lifecycleState, "inactive");
  assert.equal(activatedState.human.state.dropInterval.intervalMs, 800);
  assert.equal(activatedState.human.state.pendingDropInterval.intervalMs, 200);
  assert.equal(activatedState.human.decision.attributes.current_drop_interval_ms, 800);
  assert.notEqual(
    activatedState.human.state.pendingDropInterval.exposureId,
    activatedState.human.state.dropInterval.exposureId,
  );
  const lock = await fetch(`${gatewayUrl}/api/commands/hard-drop`, {
    method: "POST",
  });
  assert.equal(lock.status, 200);
  const appliedState = await fetchJson(`${gatewayUrl}/api/state`);
  assert.equal(appliedState.human.state.dropInterval.intervalMs, 200);
  assert.equal(appliedState.human.state.pendingDropInterval, undefined);
  assert.equal(appliedState.human.state.dropInterval.executableDigest, candidateDigest);
  assert.equal(
    appliedState.human.state.dropInterval.exposureId,
    activatedState.human.state.pendingDropInterval.exposureId,
  );

  const pause = await fetch(`${gatewayUrl}/api/commands/pause`, { method: "POST" });
  const paused = await pause.json();
  const lastKnownAnalysis = paused.timeline.find((value) =>
    value.name === "flaggo.analysis.cycle.completed"
  );
  assert.ok(lastKnownAnalysis);
  const params = new URLSearchParams({ contractDigest, cycleId: "cycle-1", attemptId: "attempt-1" });
  const inspected = await fetchJson(`${gatewayUrl}/api/analysis/actions?${params}`);
  assert.equal(inspected.actions.length, 1);
  assert.equal(inspected.actions[0].actionId, actionId);
  assert.equal(inspected.actions[0].outcome, "success");
  assert.doesNotMatch(JSON.stringify(inspected), /PRIVATE|prompt|dataset/);
  assert.ok(actionQueries.some((query) => query.includes(contractDigest)
    && query.includes("cycle-1") && query.includes("attempt-1")));
  assert.equal((await fetch(`${gatewayUrl}/api/analysis/actions?cycleId=invalid`)).status, 400);
  assert.deepEqual((await fetchJson(`${gatewayUrl}/api/state`)).human.state, paused.human.state);
  emitActivity = false;
  failLogSample = true;
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.activity.status === "stale";
  }, 5_000, () => output);
  const stale = await fetchJson(`${gatewayUrl}/api/state`);
  assert.match(stale.activity.error, /HTTP 503/);
  assert.deepEqual(stale.timeline.find((value) =>
    value.id === lastKnownAnalysis.id
  ), lastKnownAnalysis);
  assert.equal(stale.analysisActions.status, "stale");
  assert.equal(stale.analysisActions.actions[0].outcome, "success");
  emitActivity = true;
  failLogSample = false;
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.activity.status === "current";
  }, 5_000, () => output);
  const recovered = await fetchJson(`${gatewayUrl}/api/state`);
  assert.equal(recovered.timeline.filter((value) =>
    value.id === lastKnownAnalysis.id
  ).length, 1);
  assert.deepEqual(recovered.backendFailures, []);
  assert.equal(recovered.analysisActions.status, "current");
  assert.equal(recovered.analysisActions.actions.length, 1);
  const startBots = await fetch(`${gatewayUrl}/api/bots/start`, { method: "POST" });
  assert.equal(startBots.status, 202);
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.bots.every((slot) => slot.session?.decision?.source === "flaggo");
  }, 5_000, () => output);
  const playing = await fetchJson(`${gatewayUrl}/api/state`);
  assert.deepEqual(playing.human.state, paused.human.state);
  assert.equal(playing.human.decision.sessionId, state.human.state.sessionId);
  assert.equal(new Set(playing.bots.map((slot) => slot.session.state.sessionId)).size, 10);
  assert.ok(playing.bots.every((slot) =>
    slot.session.decision.sessionId === slot.session.state.sessionId));
  const duplicateStart = await fetch(`${gatewayUrl}/api/bots/bot-1/start`, { method: "POST" });
  assert.equal(duplicateStart.status, 409);
  const forbiddenDemo = await fetch(`${gatewayUrl}/api/demo/run`, { method: "POST" });
  assert.equal(forbiddenDemo.status, 409);
  const stopBots = await fetch(`${gatewayUrl}/api/bots/stop`, { method: "POST" });
  assert.equal(stopBots.status, 200);
  assert.ok((await stopBots.json()).bots.every((slot) =>
    slot.status === "idle" && slot.session.state.status === "closed"));
  const oldHumanId = playing.human.state.sessionId;
  const restart = await fetch(`${gatewayUrl}/api/commands/restart`, { method: "POST" });
  assert.notEqual((await restart.json()).human.state.sessionId, oldHumanId);

  floodActions = true;
  await waitFor(async () => {
    const current = await fetchJson(`${gatewayUrl}/api/state`);
    return current.analysisActions.observations.length === 400
      && current.analysisActions.actions.length === 200
      && !current.analysisActions.actions.some((action) => action.actionId === actionId);
  }, 5_000, () => output);
  const olderInspection = await fetchJson(`${gatewayUrl}/api/analysis/actions?${params}`);
  assert.equal(olderInspection.actions.length, 1);
  assert.equal(olderInspection.actions[0].actionId, actionId);
  assert.equal(olderInspection.actions[0].outcome, "success");
  const capped = (await fetchJson(`${gatewayUrl}/api/state`)).analysisActions;
  assert.equal(capped.observations.length, 400);
  assert.equal(capped.actions.length, 200);
  assert.equal(capped.actions.some((action) => action.actionId === actionId), false);

  const shutdown = await fetch(`${gatewayUrl}/api/shutdown`, {
    method: "POST",
  });
  assert.equal(shutdown.status, 202);
  const [code, signal] = await once(child, "close");
  assert.equal(signal, null, output);
  assert.equal(code, 0, output);
});

async function waitFor(operation, timeoutMilliseconds, diagnostics) {
  const deadline = Date.now() + timeoutMilliseconds;
  let lastError;
  while (Date.now() < deadline) {
    try {
      if (await operation()) return;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
  }
  throw new Error(
    `Condition was not met within ${timeoutMilliseconds}ms.`
    + `${lastError === undefined ? "" : ` ${String(lastError)}`}\n`
    + diagnostics(),
  );
}

async function fetchJson(url) {
  const response = await fetch(url);
  assert.equal(response.status, 200);
  return response.json();
}

async function readBody(request) {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

function json(response, status, value) {
  response.writeHead(status, {
    "Content-Type": "application/json",
  });
  response.end(JSON.stringify(value));
}

function managedExecutable(digest, lifecycleState, stateVersion) {
  return {
    contractDigest,
    executableDigest: digest,
    lifecycleState,
    stateVersion,
    createdAt: "2026-08-01T12:00:00.000Z",
    ...(lifecycleState === "active"
      ? { activatedAt: "2026-08-01T12:00:01.000Z" }
      : {}),
    executable: {
      contractDigest,
      kind: "rules",
      rules: digest === candidateDigest ? [liveContract.authoredExecutable.rules[0], {
        name: "learned-low-pressure",
        when: { expression: "attributes.board_pressure_mean_5s < 0.15" },
        return: { value: 200 },
      }, {
        name: "learned-other-context",
        when: { expression: "true" },
        return: { value: 1_000 },
      }] : liveContract.authoredExecutable.rules,
    },
    provenance: { source: lifecycleState === "candidate" ? "analysis" : "test" },
  };
}

function serverUrl(server) {
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Server did not bind an IP socket.");
  }
  return `http://127.0.0.1:${address.port}`;
}

async function reservePort() {
  const server = createNetServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Port reservation did not bind an IP socket.");
  }
  await new Promise((resolvePromise, reject) => {
    server.close((error) => {
      if (error === undefined) resolvePromise();
      else reject(error);
    });
  });
  return address.port;
}
