import assert from "node:assert/strict";
import { createServer } from "node:http";
import { rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
} from "./host-process.mjs";
import { deployTetrisContract } from "./deploy-contract.mjs";
import { startLocalFlaggoHosts } from "./local-flaggo.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `tetris-integration-${process.pid}-${Date.now()}`,
);

async function runIntegration(lifecycle) {
  const hosts = await startLocalFlaggoHosts({
    lifecycle,
    repositoryRoot,
    exampleDirectory,
    runDirectory,
  });
  const initialInbox = await readRawOtlpInboxHealth(hosts);
  assert.equal(initialInbox.retainedBatchCount, 0);
  assert.equal(initialInbox.retainedPayloadBytes, 0);
  const proxy = await startRecordingProxy(hosts.otelIngestionUrl);
  lifecycle.trackHost(proxy);
  const deployed = await deployTetrisContract({
    exampleDirectory,
    services: {
      contractServiceUrl: hosts.contractUrl,
      decisionServiceUrl: hosts.decisionUrl,
      otlpIngestionUrl: proxy.url,
    },
    fetch: hosts.fetch,
    signal: lifecycle.signal,
  });
  const capturedDecisions = [];
  const forwardingFetch = async (input, init) => {
    const isDecision = String(input).includes("/decisions");
    const request = isDecision
      ? JSON.parse(String(init?.body))
      : undefined;
    const response = await hosts.fetch(input, init);
    if (request !== undefined) {
      capturedDecisions.push({
        request,
        response: await response.clone().json(),
      });
    }
    return response;
  };

  const [
    { createFlaggoDropIntervalProvider },
    { SequencePieceSource },
    { TetrisTelemetryProviders },
    { TetrisSession },
  ] = await Promise.all([
    import("./dist/flaggo/flaggo-provider.js"),
    import("./dist/flaggo/game.js"),
    import("./dist/flaggo/otel.js"),
    import("./dist/flaggo/session.js"),
  ]);
  const telemetry = new TetrisTelemetryProviders({
    capture: true,
    runtimeConfig: deployed.runtimeConfig,
  });
  const provider = createFlaggoDropIntervalProvider({
    runtimeConfig: deployed.runtimeConfig,
    credential: { mode: "local-development" },
    fetch: forwardingFetch,
    telemetry: telemetry.policyInstrumentation,
  });
  const lowClock = deterministicClock();
  const highClock = deterministicClock();
  const lowSession = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "tetris-e2e-low",
    },
    instrumentation: telemetry.sessionInstrumentation,
    now: lowClock.now,
    provider,
  });
  const highSession = new TetrisSession({
    game: {
      pieceSource: new SequencePieceSource(["O"]),
      sessionId: "tetris-e2e-high",
    },
    instrumentation: telemetry.sessionInstrumentation,
    now: highClock.now,
    provider,
  });
  let lowPolicy;
  let highPolicy;
  let telemetrySummary;
  try {
    lowSession.dispatch("pause");
    lowSession.dispatch("resume");
    for (const horizontalOffset of [-4, -2, 0, 2, 4]) {
      moveHorizontally(lowSession, horizontalOffset);
      lowClock.advance(200);
      lowSession.dispatch("hard-drop");
    }
    assert.equal(lowSession.snapshot().lines, 2);
    lowPolicy = await lowSession.refreshPolicy(lifecycle.signal);

    highSession.dispatch("restart");
    moveHorizontally(highSession, -4);
    highSession.dispatch("move-left");
    highSession.dispatch("move-left");
    highSession.dispatch("move-left");
    highClock.advance(1_600);
    highSession.dispatch("hard-drop");
    highPolicy = await highSession.refreshPolicy(lifecycle.signal);

    assert.equal(lowPolicy.state.dropInterval.intervalMs, 750);
    assert.equal(lowPolicy.state.dropInterval.source, "flaggo");
    assert.equal(lowPolicy.policyContext?.sessionId, "tetris-e2e-low");
    assert.equal(lowPolicy.policyContext?.piecesLocked5s, 5);
    assert.equal(highPolicy.state.dropInterval.intervalMs, 850);
    assert.equal(highPolicy.state.dropInterval.source, "flaggo");
    assert.equal(highPolicy.policyContext?.sessionId, "tetris-e2e-high");
    assert.equal(highPolicy.policyContext?.recoveryFailures5s, 3);

    lowSession.close();
    highSession.close();
    await telemetry.forceFlush();
    telemetrySummary = assertTelemetry({
      telemetry,
      proxy,
      expectedSessionIds: ["tetris-e2e-low", "tetris-e2e-high"],
    });
  } finally {
    lowSession.close();
    highSession.close();
    await telemetry.shutdown();
  }
  const storedInbox = await assertRawOtlpInboxStorage({
    initial: initialInbox,
    hosts,
    requests: proxy.requests,
  });

  assert.equal(capturedDecisions.length, 2);
  const decisionsBySession = new Map(
    capturedDecisions.map((decision) => [
      decision.request.attributes.session_id,
      decision,
    ]),
  );
  const lowDecision = decisionsBySession.get("tetris-e2e-low");
  const highDecision = decisionsBySession.get("tetris-e2e-high");
  assert.ok(lowDecision);
  assert.ok(highDecision);
  assert.equal(lowDecision.response.result, 750);
  assert.deepEqual(
    lowDecision.response.evaluation,
    { source: "rule", rule: "low-pressure" },
  );
  assert.equal(highDecision.response.result, 850);
  assert.deepEqual(
    highDecision.response.evaluation,
    { source: "rule", rule: "high-pressure" },
  );
  for (const decision of capturedDecisions) {
    assert.deepEqual(Object.keys(decision.request), ["attributes"]);
    assert.equal(typeof decision.request.attributes._random, "number");
    assert.equal(
      decision.response.contractDigest,
      deployed.deployment.contractDigest,
    );
    assert.equal(
      decision.response.executableDigest,
      deployed.deployment.activeExecutableDigest,
    );
    assertNoRetiredRuntimeFields(decision.request);
    assertNoRetiredRuntimeFields(decision.response);
  }

  const restHigh = await postDecisionRest({
    fetch: hosts.fetch,
    decisionUrl: hosts.decisionUrl,
    contractName: deployed.contract.name,
    contractDigest: deployed.deployment.contractDigest,
    attributes: highDecision.request.attributes,
    signal: lifecycle.signal,
  });
  const restLow = await postDecisionRest({
    fetch: hosts.fetch,
    decisionUrl: hosts.decisionUrl,
    contractName: deployed.contract.name,
    contractDigest: deployed.deployment.contractDigest,
    attributes: lowDecision.request.attributes,
    signal: lifecycle.signal,
  });
  assertEquivalentDecision(restHigh, highDecision.response);
  assertEquivalentDecision(restLow, lowDecision.response);

  await hosts.decision.stop();
  await assert.rejects(() =>
    postDecisionRest({
      fetch: hosts.fetch,
      decisionUrl: hosts.decisionUrl,
      contractName: deployed.contract.name,
      contractDigest: deployed.deployment.contractDigest,
      attributes: highDecision.request.attributes,
      signal: lifecycle.signal,
    })
  );
  await hosts.contract.stop();
  await hosts.otelIngestion.stop();

  const restartedHosts = await startLocalFlaggoHosts({
    lifecycle,
    repositoryRoot,
    exampleDirectory,
    runDirectory,
  });
  const reopenedInbox = await readRawOtlpInboxHealth(restartedHosts);
  assert.equal(
    reopenedInbox.retainedBatchCount,
    storedInbox.retainedBatchCount,
  );
  assert.equal(
    reopenedInbox.retainedPayloadBytes,
    storedInbox.retainedPayloadBytes,
  );
  const restartedHigh = await postDecisionRest({
    fetch: restartedHosts.fetch,
    decisionUrl: restartedHosts.decisionUrl,
    contractName: deployed.contract.name,
    contractDigest: deployed.deployment.contractDigest,
    attributes: highDecision.request.attributes,
    signal: lifecycle.signal,
  });
  assertEquivalentDecision(restartedHigh, highDecision.response);

  process.stdout.write(`${JSON.stringify({
    status: "passed",
    contractDigest: deployed.deployment.contractDigest,
    executableDigest: deployed.deployment.activeExecutableDigest,
    headlessSessions: {
      high: highPolicy.state.dropInterval.intervalMs,
      low: lowPolicy.state.dropInterval.intervalMs,
      lowLinesCleared: lowPolicy.state.lines,
    },
    telemetry: telemetrySummary,
    rawOtlpInbox: {
      retainedBatches: storedInbox.retainedBatchCount,
      retainedPayloadBytes: storedInbox.retainedPayloadBytes,
      survivedRestart: true,
    },
    restParity: {
      high: restHigh.result,
      low: restLow.result,
    },
    restartPersistence: {
      high: restartedHigh.result,
      executableDigest: restartedHigh.executableDigest,
    },
    retiredRuntimeFields: "absent",
  }, null, 2)}\n`);
}

function assertTelemetry({ telemetry, proxy, expectedSessionIds }) {
  const events = telemetry.events;
  const eventNames = new Set(events.map((event) => event.eventName));
  for (const expected of [
    "flaggo.decision.received",
    "tetris.drop_interval.selected",
    "tetris.game.paused",
    "tetris.game.restarted",
    "tetris.game.resumed",
    "tetris.game.started",
    "tetris.lines.cleared",
    "tetris.piece.locked",
    "tetris.piece.spawned",
    "tetris.recovery.failed",
  ]) {
    assert.ok(eventNames.has(expected), `missing telemetry event '${expected}'`);
  }
  assert.ok(events.every((event) =>
    event.resource.attributes["service.name"] === "tetris"
    && event.resource.attributes["deployment.environment.name"] === "integration"
    && event.resource.attributes["flaggo.tenant"] === "local"
    && event.resource.attributes["flaggo.application"] === "tetris"
    && event.resource.attributes["flaggo.environment"] === "integration"
  ));
  assert.ok(
    events
      .filter((event) => event.eventName === "tetris.drop_interval.selected")
      .every((event) =>
        !Object.hasOwn(event.attributes, "flaggo.contract.name")
        && !Object.hasOwn(event.attributes, "flaggo.contract.digest")
      ),
  );
  assert.ok(
    events
      .filter((event) => event.eventName?.startsWith("tetris.game.") === true
        || event.eventName?.startsWith("tetris.piece.") === true
        || event.eventName === "tetris.lines.cleared"
        || event.eventName === "tetris.recovery.failed")
      .every((event) => event.instrumentationScope.name === "tetris.engine"),
  );
  assert.ok(
    events
      .filter((event) => event.eventName === "tetris.drop_interval.selected")
      .every((event) => event.instrumentationScope.name === "tetris.policy"),
  );
  assert.ok(
    events
      .filter((event) => event.eventName === "flaggo.decision.received")
      .every((event) => event.instrumentationScope.name === "@flaggo/sdk"),
  );
  assert.ok(events.every((event) =>
    !Object.hasOwn(event.attributes, "tetris.board")
  ));

  const spans = telemetry.spans;
  const commandSpans = spans.filter((span) => span.name === "tetris.command");
  const policySpans = spans.filter((span) =>
    span.name === "tetris.drop_interval.select"
  );
  assert.ok(commandSpans.length > 0);
  assert.equal(policySpans.length, 2);
  assert.ok(commandSpans.every((span) =>
    span.instrumentationScope.name === "tetris.engine"
  ));
  assert.ok(policySpans.every((span) =>
    span.instrumentationScope.name === "tetris.policy"
  ));
  const commandTraceIds = new Set(
    commandSpans.map((span) => span.spanContext().traceId),
  );
  const policyTraceIds = new Set(
    policySpans.map((span) => span.spanContext().traceId),
  );
  assert.ok(
    events
      .filter((event) => event.eventName === "tetris.piece.locked")
      .every((event) =>
        event.spanContext !== undefined
        && commandTraceIds.has(event.spanContext.traceId)
      ),
  );
  assert.ok(
    events
      .filter((event) =>
        event.eventName === "tetris.drop_interval.selected"
        || event.eventName === "flaggo.decision.received"
      )
      .every((event) =>
        event.spanContext !== undefined
        && policyTraceIds.has(event.spanContext.traceId)
      ),
  );

  const metrics = telemetry.metrics.flatMap((resourceMetrics) =>
    resourceMetrics.scopeMetrics.flatMap((scope) =>
      scope.metrics.map((metric) => ({ metric, scope: scope.scope.name }))
    )
  );
  const placement = metric(metrics, "tetris.placement_time");
  const recovery = metric(metrics, "tetris.recovery_failure");
  const score = metric(metrics, "tetris.score");
  assert.equal(placement.scope, "tetris.engine");
  assert.equal(recovery.scope, "tetris.engine");
  assert.equal(score.scope, "tetris.engine");
  assert.deepEqual(
    new Set(
      placement.metric.dataPoints.map(
        (point) => point.attributes["tetris.session.id"],
      ),
    ),
    new Set(expectedSessionIds),
  );
  assert.ok(
    recovery.metric.dataPoints.some((point) =>
      point.attributes["tetris.session.id"] === "tetris-e2e-high"
    ),
  );
  assert.deepEqual(
    new Set(
      score.metric.dataPoints.map(
        (point) => point.attributes["tetris.session.id"],
      ),
    ),
    new Set(expectedSessionIds),
  );
  assert.ok(metrics.some(({ metric: value, scope }) =>
    value.descriptor.name === "tetris.board_pressure_mean_5s"
    && scope === "tetris.policy"
  ));

  assert.ok(proxy.requests.length >= 3);
  assert.deepEqual(
    new Set(proxy.requests.map((request) => request.path)),
    new Set(["/v1/logs", "/v1/metrics", "/v1/traces"]),
  );
  assert.ok(proxy.requests.every((request) =>
    request.contentType === "application/json"
    && request.status === 200
  ));
  return {
    events: events.length,
    metrics: new Set(
      metrics.map(({ metric: value }) => value.descriptor.name),
    ).size,
    requests: proxy.requests.length,
    spans: spans.length,
  };
}

function metric(metrics, name) {
  const found = metrics.find(({ metric: candidate }) =>
    candidate.descriptor.name === name
  );
  assert.ok(found, `missing metric '${name}'`);
  return found;
}

function deterministicClock() {
  let current = 0;
  return {
    now: () => current,
    advance(milliseconds) {
      current += milliseconds;
    },
  };
}

function moveHorizontally(session, offset) {
  const command = offset < 0 ? "move-left" : "move-right";
  for (let count = 0; count < Math.abs(offset); count += 1) {
    assert.equal(session.dispatch(command).changed, true);
  }
}

async function assertRawOtlpInboxStorage({ initial, hosts, requests }) {
  assert.ok(requests.length >= 3);
  assert.deepEqual(
    new Set(requests.map((request) => request.path)),
    new Set(["/v1/logs", "/v1/metrics", "/v1/traces"]),
  );
  assert.ok(requests.every((request) =>
    request.contentEncoding === undefined
    && request.contentType === "application/json"
    && Number.isInteger(request.payloadLength)
    && request.payloadLength > 0
    && request.status === 200
  ));
  const expectedPayloadBytes = requests.reduce(
    (total, request) => total + request.payloadLength,
    0,
  );
  const stored = await readRawOtlpInboxHealth(hosts);
  assert.equal(
    stored.retainedBatchCount - initial.retainedBatchCount,
    requests.length,
  );
  assert.equal(
    stored.retainedPayloadBytes - initial.retainedPayloadBytes,
    expectedPayloadBytes,
  );
  assert.equal(stored.expiredBatchCount, 0);
  assert.equal(stored.expiredPayloadBytes, 0);
  assert.ok(stored.earliestReplayAt);
  assert.ok(stored.newestRetainedAt);
  assert.ok(stored.oldestRetainedAt);
  return stored;
}

async function readRawOtlpInboxHealth(hosts) {
  const response = await hosts.fetch(
    `${hosts.otelIngestionUrl}/health/ready`,
  );
  const body = await response.json();
  assert.equal(response.ok, true, JSON.stringify(body));
  assert.equal(body.status, "ready");
  for (const property of [
    "expiredBatchCount",
    "expiredPayloadBytes",
    "retainedBatchCount",
    "retainedPayloadBytes",
  ]) {
    assert.equal(
      Number.isSafeInteger(body.inbox[property]),
      true,
      `inbox '${property}' must be a safe integer`,
    );
  }
  return body.inbox;
}

async function postDecisionRest({
  fetch,
  decisionUrl,
  contractName,
  contractDigest,
  attributes,
  signal,
}) {
  const url = `${decisionUrl}/v3/decision-contracts/${
    encodeURIComponent(contractName)
  }/versions/${encodeURIComponent(contractDigest)}/decisions`;
  const request = { attributes };
  assertNoRetiredRuntimeFields(request);
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Accept: "application/json",
      Authorization: "Flaggo-Local-Development",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(request),
    signal,
  });
  const body = await response.json();
  assert.equal(response.ok, true, JSON.stringify(body));
  assertNoRetiredRuntimeFields(body);
  return body;
}

function assertEquivalentDecision(actual, expected) {
  assert.equal(actual.result, expected.result);
  assert.deepEqual(actual.evaluation, expected.evaluation);
  assert.equal(actual.contractDigest, expected.contractDigest);
  assert.equal(actual.executableDigest, expected.executableDigest);
}

function assertNoRetiredRuntimeFields(value) {
  const retiredFields = new Set([
    "authoredExecutable",
    "confidence",
    "confirmation",
    "confirmationRequired",
    "confirmationToken",
    "contract",
    "evidence",
    "EvidenceSnapshot",
    "evidenceSnapshot",
    "exposure",
    "exposureId",
    "exposureToken",
    "fallback",
    "idempotencyKey",
    "runtimeTarget",
    "staticDefinition",
  ]);
  const pending = [value];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === null || typeof current !== "object") continue;
    if (Array.isArray(current)) {
      pending.push(...current);
      continue;
    }
    for (const [key, child] of Object.entries(current)) {
      assert.equal(
        retiredFields.has(key),
        false,
        `unexpected retired field '${key}'`,
      );
      pending.push(child);
    }
  }
}

async function startRecordingProxy(upstreamBaseUrl) {
  const requests = [];
  const server = createServer((request, response) => {
    void forward(request, response).catch((error) => {
      requests.push({
        path: request.url ?? "/",
        contentType: request.headers["content-type"],
        status: 502,
      });
      response.writeHead(502, { "Content-Type": "text/plain" });
      response.end(error instanceof Error ? error.message : String(error));
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.removeListener("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("Telemetry proxy did not bind a TCP port.");
  }

  async function forward(request, response) {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const path = request.url ?? "/";
    const contentType = request.headers["content-type"];
    const contentEncoding = request.headers["content-encoding"];
    const upstream = await fetch(new URL(path, upstreamBaseUrl), {
      method: request.method,
      headers: {
        ...(contentType === undefined ? {} : { "content-type": contentType }),
        ...(contentEncoding === undefined
          ? {}
          : { "content-encoding": contentEncoding }),
      },
      body,
    });
    const upstreamBody = Buffer.from(await upstream.arrayBuffer());
    requests.push({
      path,
      contentEncoding,
      contentType,
      payloadLength: body.length,
      status: upstream.status,
    });
    response.writeHead(upstream.status, {
      "Content-Type": upstream.headers.get("content-type")
        ?? "application/json",
    });
    response.end(upstreamBody);
  }

  let stopPromise;
  return {
    name: "tetris-otel-proxy",
    requests,
    url: `http://127.0.0.1:${address.port}`,
    get exited() {
      return !server.listening;
    },
    get unexpectedExit() {
      return undefined;
    },
    stop() {
      stopPromise ??= new Promise((resolve, reject) => {
        server.close((error) => error === undefined ? resolve() : reject(error));
      });
      return stopPromise;
    },
  };
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () =>
      rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => runIntegration(lifecycle),
      lifecycle,
      (cleanupError) => {
        process.stderr.write(`Cleanup failure: ${formatError(cleanupError)}\n`);
      },
    );
  } catch (error) {
    if (lifecycle.signalExitCode !== undefined) {
      process.exitCode = lifecycle.signalExitCode;
    } else {
      process.exitCode = 1;
      process.stderr.write(`${formatError(error)}\n`);
    }
  } finally {
    uninstallSignalHandlers();
  }
}

function formatError(error) {
  return error instanceof Error
    ? error.stack ?? error.message
    : String(error);
}

await main();
