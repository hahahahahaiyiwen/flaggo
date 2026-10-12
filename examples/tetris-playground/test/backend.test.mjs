import assert from "node:assert/strict";
import test from "node:test";

import {
  LgtmClient,
  parseLokiResponse,
  parsePrometheusResponse,
  parseTempoResponse,
} from "../dist/backend.js";

test("queries both ends of the bounded Loki range", async () => {
  const requests = [];
  const client = new LgtmClient({
    lokiUrl: "http://loki.test",
    prometheusUrl: "http://prometheus.test",
    tempoUrl: "http://tempo.test",
  }, async (input) => {
    const url = new URL(input);
    requests.push(url);
    if (url.pathname === "/loki/api/v1/query_range") {
      return jsonResponse({
        status: "success",
        data: { resultType: "streams", result: [] },
      });
    }
    if (url.pathname === "/api/search") {
      return jsonResponse({ traces: [] });
    }
    return jsonResponse({
      status: "success",
      data: { resultType: "vector", result: [] },
    });
  });

  await client.query(new Date("2025-10-09T08:53:20.123Z"));

  assert.deepEqual(
    requests
      .filter((url) => url.pathname === "/loki/api/v1/query_range"
        && url.searchParams.get("query") === '{service_name=~"flaggo-.+"}')
      .map((url) => url.searchParams.get("direction"))
      .sort(),
    ["backward", "forward"],
  );
  const activityRequests = requests.filter((url) =>
    url.pathname === "/loki/api/v1/query_range"
    && url.searchParams.get("query").includes("event_name")
    && !url.searchParams.get("query").includes("action")
  );
  assert.equal(activityRequests.length, 2);
  assert.ok(activityRequests.every((url) =>
    url.searchParams.get("direction") === "backward"
    && url.searchParams.get("limit") === "60"
  ));
});

test("a stalled backend times out without blocking the other observed sources", async () => {
  let aborted = false;
  const client = new LgtmClient({
    lokiUrl: "http://loki.test",
    prometheusUrl: "http://prometheus.test",
    tempoUrl: "http://tempo.test",
  }, async (input, init) => {
    const url = new URL(input);
    if (url.pathname === "/api/search") {
      return new Promise((_, reject) => {
        init.signal.addEventListener("abort", () => {
          aborted = true;
          reject(init.signal.reason);
        }, { once: true });
      });
    }
    return url.pathname === "/loki/api/v1/query_range"
      ? jsonResponse({ status: "success", data: { resultType: "streams", result: [] } })
      : jsonResponse({ status: "success", data: { resultType: "vector", result: [] } });
  });
  const keepAlive = setInterval(() => {}, 100);
  try {
    const result = await client.query(new Date());
    assert.equal(aborted, true);
    assert.equal(result.failures.length, 1);
    assert.equal(result.failures[0].source, "tempo");
    assert.match(result.failures[0].message, /timeout/iu);
  } finally {
    clearInterval(keepAlive);
  }
});

test("retains successful Loki samples and reports each failed sample", async () => {
  let failBroad = false;
  const client = new LgtmClient({
    lokiUrl: "http://loki.test",
    prometheusUrl: "http://prometheus.test",
    tempoUrl: "http://tempo.test",
  }, async (input) => {
    const url = new URL(input);
    if (url.pathname === "/loki/api/v1/query_range") {
      if (failBroad && !url.searchParams.get("query").includes("event_name")) {
        return new Response("unavailable", { status: 503 });
      }
      return jsonResponse({
        status: "success",
        data: { resultType: "streams", result: [{
          stream: { service_name: "flaggo-async-analysis" },
          values: [["1760000000123000000", "analysis", {
            event_name: "flaggo.analysis.cycle.completed",
            otel_scope_name: "flaggo.async-analysis",
          }]],
        }] },
      });
    }
    return url.pathname === "/api/search"
      ? jsonResponse({ traces: [] })
      : jsonResponse({ status: "success", data: { resultType: "vector", result: [] } });
  });
  const initial = await client.query(new Date());
  assert.equal(initial.logs.length, 1);
  assert.deepEqual(initial.failures, []);
  failBroad = true;
  const partial = await client.query(new Date());
  assert.deepEqual(partial.logs, initial.logs);
  assert.equal(partial.failures.length, 2);
  assert.ok(partial.failures.every((failure) =>
    failure.source === "loki" && failure.message.includes("HTTP 503")
  ));
  failBroad = false;
  const recovered = await client.query(new Date());
  assert.deepEqual(recovered.logs, initial.logs);
  assert.deepEqual(recovered.failures, []);
});

test("normalizes Loki labels and structured metadata to canonical attributes", () => {
  const observedAtNanoseconds = "1760000000123000000";
  const logs = parseLokiResponse({
    status: "success",
    data: {
      resultType: "streams",
      result: [{
        stream: {
          service_name: "flaggo-contract-service",
          otel_scope_name: "flaggo.contract-service",
        },
        values: [[
          observedAtNanoseconds,
          JSON.stringify({
            event_name: "flaggo.activation.completed",
            flaggo_operation_outcome: "success",
          }),
          {
            flaggo_contract_name: "tetris.dropInterval",
            trace_id: "trace-1",
          },
        ]],
      }],
    },
  });

  assert.equal(logs.length, 1);
  assert.equal(logs[0].service, "flaggo-contract-service");
  assert.equal(logs[0].eventName, "flaggo.activation.completed");
  assert.equal(logs[0].traceId, "trace-1");
  assert.equal(
    logs[0].attributes["flaggo.operation.outcome"],
    "success",
  );
  assert.equal(
    logs[0].attributes["otel.scope.name"],
    "flaggo.contract-service",
  );
  assert.equal(
    logs[0].attributes["flaggo.contract.name"],
    "tetris.dropInterval",
  );
  assert.equal(logs[0].observedAt, "2025-10-09T08:53:20.123Z");
});

test("keeps recent activity when both broad Loki samples are saturated", async () => {
  const base = Date.parse("2025-10-09T08:53:20.000Z");
  const entries = Array.from({ length: 4_295 }, (_, index) => ({
    at: String(BigInt(base + index) * 1_000_000n),
    service: "flaggo-evidence-materializer",
    name: "flaggo.materializer.page.committed",
    outcome: "success",
  }));
  for (const index of [0, 1, 2_102, 2_103,
    ...Array.from({ length: 90 }, (_, offset) => entries.length - 90 + offset)]) {
    entries[index].service = "flaggo-async-analysis";
    entries[index].name = "flaggo.analysis.cycle.completed";
    entries[index].outcome = index === 2_102 || index === 2_103 ? "no_candidate" : "not_eligible";
  }
  const client = new LgtmClient({
    lokiUrl: "http://loki.test",
    prometheusUrl: "http://prometheus.test",
    tempoUrl: "http://tempo.test",
  }, async (input) => {
    const url = new URL(input);
    if (url.pathname === "/loki/api/v1/query_range") {
      const query = url.searchParams.get("query");
      let selected = entries;
      if (query.includes("event_name")) {
        selected = selected.filter((entry) => entry.service === "flaggo-async-analysis");
      }
      if (query.includes('!= "not_eligible"')) {
        selected = selected.filter((entry) => entry.outcome !== "not_eligible");
      }
      if (url.searchParams.get("direction") === "backward") selected = [...selected].reverse();
      selected = selected.slice(0, Number(url.searchParams.get("limit")));
      return jsonResponse({
        status: "success",
        data: { resultType: "streams", result: selected.map((entry) => ({
          stream: { service_name: entry.service },
          values: [[entry.at, "Operational event", {
            event_name: entry.name,
            otel_scope_name: entry.service.replace("flaggo-", "flaggo."),
            flaggo_operation_outcome: entry.outcome,
          }]],
        })) },
      });
    }
    if (url.pathname === "/api/search") return jsonResponse({ traces: [] });
    return jsonResponse({ status: "success", data: { resultType: "vector", result: [] } });
  });

  const result = await client.query(new Date(base));
  assert.deepEqual(
    result.logs.filter((entry) => entry.attributes["flaggo.operation.outcome"] === "no_candidate")
      .map((entry) => entry.observedAt).sort(),
    [2_102, 2_103].map((index) => new Date(base + index).toISOString()),
  );
  assert.equal(new Set(result.logs.map((entry) => entry.id)).size, result.logs.length);
  assert.deepEqual(result.failures, []);
});

test("normalizes Tempo span sets without depending on backend-only fields", () => {
  const spans = parseTempoResponse({
    traces: [{
      traceID: "trace-2",
      rootServiceName: "flaggo-decision-service",
      spanSets: [{
        spans: [{
          spanID: "span-2",
          name: "flaggo.decision.evaluate",
          startTimeUnixNanos: "1760000000123000000",
          durationNanos: 2_500_000,
          scopeName: "flaggo.decision-service",
          attributes: [
            {
              key: "flaggo.operation.outcome",
              value: { stringValue: "success" },
            },
          ],
        }],
      }],
    }],
  });

  assert.deepEqual(spans, [{
    id: "span:trace-2:span-2:1760000000123000000",
    observedAt: "2025-10-09T08:53:20.123Z",
    service: "flaggo-decision-service",
    name: "flaggo.decision.evaluate",
    durationMs: 2.5,
    traceId: "trace-2",
    spanId: "span-2",
    attributes: {
      "flaggo.operation.outcome": "success",
      "otel.scope.name": "flaggo.decision-service",
    },
  }]);
});

test("normalizes Prometheus vectors to metric view models", () => {
  const metrics = parsePrometheusResponse({
    status: "success",
    data: {
      resultType: "vector",
      result: [{
        metric: {
          __name__: "flaggo_decision_requests_total",
          service_name: "flaggo-decision-service",
          flaggo_operation_outcome: "success",
        },
        value: [1760000000, "7"],
      }],
    },
  });

  assert.deepEqual(metrics, [{
    name: "flaggo_decision_requests_total",
    service: "flaggo-decision-service",
    value: 7,
    labels: {
      service_name: "flaggo-decision-service",
      flaggo_operation_outcome: "success",
    },
  }]);
});

function jsonResponse(value) {
  return new Response(JSON.stringify(value), {
    headers: { "Content-Type": "application/json" },
    status: 200,
  });
}
