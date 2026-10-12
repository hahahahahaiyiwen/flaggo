import assert from "node:assert/strict";
import test from "node:test";
import { actionObservation, createAnalysisActionsView } from "../dist/analysis-actions.js";
import { createObserverView } from "../dist/observer.js";
import { LgtmClient, parseLokiResponse } from "../dist/backend.js";

const now = new Date("2026-10-12T19:00:00.000Z");
const digest = `sha256:${"a".repeat(64)}`;
const actionId = "00000000-0000-4000-8000-000000000001";
const selection = { contractDigest: digest, cycleId: "cycle-1", attemptId: "attempt-1" };
function log(event = "started", overrides = {}) {
  return {
    id: event, observedAt: now.toISOString(), service: "flaggo-async-analysis",
    eventName: `flaggo.analysis.action.${event}`, message: "PRIVATE",
    traceId: "0123456789abcdef0123456789abcdef",
    attributes: {
      "otel.scope.name": "flaggo.async-analysis",
      "flaggo.contract.name": "tetris.dropInterval",
      "flaggo.contract.digest": digest,
      "flaggo.analysis.cycle.id": "cycle-1",
      "flaggo.analysis.attempt.id": "attempt-1",
      "flaggo.analysis.action.id": actionId,
      "flaggo.analysis.action.kind": "evidence.sql",
      "flaggo.analysis.action.outcome": event === "started" ? "running" : "success",
      ...(event === "completed" ? { "flaggo.analysis.action.duration_ms": 12 } : {}),
      ...overrides,
    },
  };
}
function view(logs, extra = {}) { return createAnalysisActionsView({ logs, now, ...extra }); }

test("pairs full correlation, deduplicates exact telemetry copies, and accepts out-of-order input", () => {
  const start = { ...log(), observedAt: "2026-10-12T18:59:59.000Z" };
  const terminal = log("completed", {
    "flaggo.analysis.result.rows": "2", "flaggo.analysis.result.bytes": 42,
    "flaggo.analysis.query.digest": digest, "flaggo.analysis.snapshot.watermark": "0",
    "flaggo.analysis.snapshot.cutoff": "2026-10-12T18:59:00Z",
  });
  const result = view([terminal, start, { ...terminal, id: "collector-replay" }]);
  assert.equal(result.observations.length, 2);
  assert.equal(result.actions.length, 1);
  const action = result.actions[0];
  assert.equal(action.startedAt, start.observedAt);
  assert.equal(action.completedAt, terminal.observedAt);
  assert.equal(action.outcome, "success");
  assert.equal(action.attributes["flaggo.analysis.result.rows"], 2);
  assert.equal(action.attributes["flaggo.analysis.action.duration_ms"], 12);
  assert.equal(action.traceId, terminal.traceId);
  assert.equal(view([
    start, log("completed", { "flaggo.analysis.attempt.id": "retry-2" }),
    log("completed", { "flaggo.analysis.cycle.id": "cycle-2" }),
    log("completed", { "flaggo.contract.digest": `sha256:${"b".repeat(64)}` }),
  ]).actions.length, 4);
});

test("partial and terminal-only histories never invent start/completion timestamps", () => {
  const incomplete = view([log()]).actions[0];
  assert.equal(incomplete.status, "completion not observed");
  assert.equal(incomplete.outcome, undefined);
  assert.equal(incomplete.completedAt, undefined);
  const terminal = view([log("completed")]).actions[0];
  assert.equal(terminal.startedAt, undefined);
  assert.equal(terminal.outcome, "success");
  const recovered = view([log()], { previous: view([log("completed")]) });
  assert.equal(recovered.actions[0].status, "completed");
});

test("contradictory terminals stay unknown rather than picking success", () => {
  const result = view([log("completed"), log("completed", {
    "flaggo.analysis.action.outcome": "failed",
  })]);
  assert.equal(result.actions[0].status, "unknown/conflicting");
  assert.equal(result.actions[0].outcome, undefined);
  assert.deepEqual(result.actions[0].attributes, {});
  const later = view([log("completed")], { previous: result });
  assert.equal(later.actions[0].status, "unknown/conflicting");
});

test("only trusted bounded allowlisted facts cross the observer boundary", () => {
  const terminal = log("completed", {
    sql: "SELECT PRIVATE", prompt: "PRIVATE", path: "PRIVATE", error: "PRIVATE",
    reasoning: "PRIVATE", summary: "PRIVATE",
    "flaggo.analysis.query.digest": "PRIVATE",
    "flaggo.analysis.session.id": "raw-provider-session",
    "flaggo.analysis.snapshot.cutoff": "PRIVATE",
    "flaggo.analysis.snapshot.watermark": -1,
    "flaggo.analysis.result.rows": 1.5,
    "flaggo.analysis.result.bytes": Infinity,
    "flaggo.analysis.tool_call.id": "x".repeat(129),
    "flaggo.failure.category": "private error string",
  });
  const result = view([terminal]);
  assert.equal(result.actions.length, 1);
  assert.doesNotMatch(JSON.stringify(result), /PRIVATE|raw-provider-session|private error/);
  for (const invalid of [
    { ...terminal, service: "attacker" },
    log("completed", { "otel.scope.name": "untrusted" }),
    log("completed", { "otel.scope.name": undefined }),
    log("completed", { "flaggo.analysis.action.id": "not-a-uuid" }),
    log("completed", { "flaggo.analysis.action.kind": "private.thinking" }),
    log("completed", { "flaggo.analysis.action.outcome": "private outcome" }),
    log("completed", { "flaggo.analysis.action.duration_ms": -1 }),
    log("completed", { "flaggo.analysis.action.duration_ms": undefined }),
    log("started", { "flaggo.analysis.action.outcome": "success" }),
    log("completed", { "flaggo.analysis.cycle.id": "x".repeat(129) }),
  ]) assert.equal(actionObservation(invalid), undefined);
});

test("bounded history exposes truncation and selection rather than claiming completeness", () => {
  const logs = Array.from({ length: 450 }, (_, index) => log("completed", {
    "flaggo.analysis.action.id": `00000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
  }));
  const result = view(logs);
  assert.equal(result.observations.length, 400);
  assert.equal(result.actions.length, 200);
  assert.equal(result.limited, true);
  assert.equal(view(logs, { queryLimited: true }).limited, true);
  assert.equal(view([log(), log("completed", { "flaggo.analysis.attempt.id": "other" })],
    { selection }).actions.length, 1);
});

test("scoped inspection survives global eviction and merges only matching observations", () => {
  const terminal = log("completed");
  const newer = Array.from({ length: 400 }, (_, index) => ({
    ...log("completed", {
      "flaggo.analysis.action.id": `10000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
      "flaggo.analysis.cycle.id": "newer-cycle",
      "flaggo.analysis.attempt.id": "newer-attempt",
    }),
    observedAt: new Date(now.getTime() + index + 1).toISOString(),
  }));
  const global = view([terminal, ...newer]);
  assert.equal(global.observations.length, 400);
  assert.equal(global.actions.length, 200);
  assert.equal(global.actions.some((action) => action.actionId === actionId), false);
  const inspected = view([terminal], { previous: global, selection });
  assert.equal(inspected.actions.length, 1);
  assert.equal(inspected.actions[0].actionId, actionId);
  const merged = view([], { previous: inspected, observations: global.observations, selection });
  assert.deepEqual(merged.actions, inspected.actions);
  const stale = view([], { previous: merged, selection, error: "Loki unavailable" });
  assert.deepEqual(stale.actions, inspected.actions);
  assert.equal(stale.status, "stale");
  const conflicting = view([], { previous: stale, selection,
    observations: view([log("completed", { "flaggo.analysis.action.outcome": "failed" })]).observations });
  assert.equal(conflicting.actions[0].status, "unknown/conflicting");
  assert.equal(conflicting.actions[0].outcome, undefined);
});

test("observer preserves facts and outcome during Loki outage and late recovery", () => {
  function observe(logs, failures = [], previous) {
    return createObserverView({
      telemetry: { logs, spans: [], metrics: [], failures },
      processes: {}, readiness: {}, now,
      ...(previous === undefined ? {} : { previous }),
    });
  }
  const initial = observe([log()]);
  const stale = observe([], [{ source: "loki", message: "503", observedAt: now.toISOString() }], initial);
  assert.equal(stale.analysisActions.status, "stale");
  assert.deepEqual(stale.analysisActions.actions, initial.analysisActions.actions);
  assert.equal(stale.analysisActions.lastSuccessfulPollAt, initial.analysisActions.lastSuccessfulPollAt);
  const recovered = observe([log("completed")], [], stale);
  assert.equal(recovered.analysisActions.status, "current");
  assert.equal(recovered.analysisActions.actions[0].outcome, "success");
  assert.equal(recovered.analysisActions.actions[0].startedAt, log().observedAt);
});

test("scoped Loki query uses bounded supported API and rejects query injection", async () => {
  const requests = [];
  const client = new LgtmClient({
    lokiUrl: "http://loki.test", prometheusUrl: "http://prom.test", tempoUrl: "http://tempo.test",
  }, async (url) => {
    requests.push(new URL(url));
    return Response.json({ status: "success", data: { result: [] } });
  });
  await client.queryAnalysisActions(new Date(0), selection);
  const url = requests[0];
  assert.equal(url.pathname, "/loki/api/v1/query_range");
  assert.equal(url.searchParams.get("limit"), "400");
  assert.equal(url.searchParams.get("direction"), "backward");
  assert.ok(BigInt(url.searchParams.get("end")) - BigInt(url.searchParams.get("start")) <= 3_600_000_000_000n);
  const query = url.searchParams.get("query");
  for (const value of [digest, "cycle-1", "attempt-1"]) assert.ok(query.includes(value));
  await assert.rejects(client.queryAnalysisActions(now, { ...selection, cycleId: '"} | malicious' }));
  assert.equal(requests.length, 1);
});

test("Loki normalized metadata preserves different simultaneous action IDs", () => {
  const entries = [actionId, "00000000-0000-4000-8000-000000000002"];
  const logs = parseLokiResponse({ data: { result: [{
    stream: { service_name: "flaggo-async-analysis" },
    values: entries.map((id) => ["1760000000000000000", "PRIVATE", {
      event_name: "flaggo.analysis.action.completed", scope_name: "flaggo.async-analysis",
      flaggo_contract_name: "tetris.dropInterval", flaggo_contract_digest: digest,
      flaggo_analysis_cycle_id: "cycle-1", flaggo_analysis_attempt_id: "attempt-1",
      flaggo_analysis_action_id: id, flaggo_analysis_action_kind: "artifact.read",
      flaggo_analysis_action_outcome: "success", flaggo_analysis_action_duration_ms: "1",
    }]),
  }] } });
  assert.notEqual(logs[0].id, logs[1].id);
  assert.equal(view(logs).actions.length, 2);
});

test("reserved action sample survives noisy broad logs and reports query saturation", async () => {
  const client = new LgtmClient({
    lokiUrl: "http://loki.test", prometheusUrl: "http://prom.test", tempoUrl: "http://tempo.test",
  }, async (input) => {
    const url = new URL(input);
    if (url.pathname !== "/loki/api/v1/query_range") {
      return Response.json(url.pathname === "/api/search" ? { traces: [] }
        : { status: "success", data: { result: [] } });
    }
    const actionQuery = url.searchParams.get("query").includes("action");
    const events = actionQuery
      ? Array.from({ length: 400 }, (_, index) => ({
        ...log("completed", {
          "flaggo.analysis.action.id": `00000000-0000-4000-8000-${index.toString().padStart(12, "0")}`,
        }).attributes,
        "event.name": "flaggo.analysis.action.completed",
      })) : [{ "event.name": "flaggo.materializer.page.committed" }];
    return Response.json({ status: "success", data: { result: [{
      stream: { service_name: "flaggo-async-analysis" },
      values: events.map((attributes) => ["1760000000000000000", "PRIVATE", attributes]),
    }] } });
  });
  const result = await client.query(now);
  assert.equal(result.actionQueryLimited, true);
  assert.equal(result.logs.filter((log) => log.eventName === "flaggo.analysis.action.completed").length, 400);
  const observed = createObserverView({ telemetry: result, now, processes: {}, readiness: {} });
  assert.equal(observed.analysisActions.actions.length, 200);
  assert.equal(observed.analysisActions.limited, true);
  assert.doesNotMatch(JSON.stringify(observed.analysisActions), /PRIVATE/);
});

test("backend HTTP errors do not expose raw response bodies", async () => {
  const client = new LgtmClient({
    lokiUrl: "http://loki.test", prometheusUrl: "http://prom.test", tempoUrl: "http://tempo.test",
  }, async () => new Response("PRIVATE SQL/provider error details", { status: 503 }));
  const result = await client.query(now);
  assert.ok(result.failures.length > 0);
  assert.doesNotMatch(JSON.stringify(result.failures), /PRIVATE|SQL|provider/);
});
