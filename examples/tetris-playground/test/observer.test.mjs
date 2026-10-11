import assert from "node:assert/strict";
import test from "node:test";

import { createObserverView } from "../dist/observer.js";

const now = new Date("2025-10-09T08:53:25.000Z");
const current = "2025-10-09T08:53:20.123Z";
const serviceNames = [
  "flaggo-contract-service",
  "flaggo-decision-service",
  "flaggo-otel-ingestion",
  "flaggo-evidence-materializer",
  "flaggo-async-analysis",
];

test("recognizes only registered lifecycle events and operation spans", () => {
  const view = createObserverView({
    telemetry: {
      logs: [
        log("known", "flaggo-async-analysis", "flaggo.analysis.cycle.completed"),
        log("unknown", "flaggo-async-analysis", "flaggo.made.up.event"),
        log(
          "wrong-service",
          "flaggo-decision-service",
          "flaggo.analysis.cycle.completed",
        ),
      ],
      spans: [{
        id: "decision-span",
        observedAt: current,
        service: "flaggo-decision-service",
        name: "flaggo.decision.evaluate",
        durationMs: 1.5,
        traceId: "trace-1",
        attributes: {
          "otel.scope.name": "flaggo.decision-service",
          "flaggo.operation.outcome": "success",
          "unregistered.attribute": "ignored",
        },
      }, {
        id: "unknown-span",
        observedAt: current,
        service: "flaggo-decision-service",
        name: "flaggo.unknown.operation",
        traceId: "trace-2",
        attributes: {},
      }],
      metrics: [{
        name: "flaggo_decision_evaluations_total",
        service: "flaggo-decision-service",
        value: 7,
        labels: {
          flaggo_operation_outcome: "success",
          contract_name: "must-not-leak",
        },
      }, {
        name: "flaggo_unregistered_total",
        value: 1,
        labels: {},
      }],
      failures: [],
    },
    processes: processStates(true),
    readiness: readyStates(),
    now,
  });

  assert.deepEqual(
    view.timeline.map((event) => event.id).sort(),
    ["decision-span", "known"],
  );
  assert.equal(
    view.services.find((service) =>
      service.name === "flaggo-async-analysis"
    )?.status,
    "healthy",
  );
  assert.equal(
    Object.hasOwn(
      view.timeline.find((event) => event.id === "decision-span").attributes,
      "unregistered.attribute",
    ),
    false,
  );
  assert.deepEqual(view.metrics, [{
    name: "flaggo_decision_evaluations_total",
    service: "flaggo-decision-service",
    value: 7,
    labels: {
      "flaggo.operation.outcome": "success",
    },
  }]);
});

test("projects materializer and inbox progress without reading storage", () => {
  const view = createObserverView({
    telemetry: {
      logs: [{
        ...log(
          "materialized",
          "flaggo-evidence-materializer",
          "flaggo.materializer.page.committed",
        ),
        attributes: {
            "otel.scope.name": "flaggo.evidence-materializer",
          "event.name": "flaggo.materializer.page.committed",
          "flaggo.materializer.checkpoint_batch_id": "42",
          "flaggo.materializer.pending_batch_count": 0,
          "flaggo.materializer.evidence_freshness_ms": 300,
          "flaggo.materializer.evidence.observation_count": 12,
          "flaggo.materializer.evidence.provenance_count": 12,
          "flaggo.materializer.batches_read": 2,
          "flaggo.materializer.observations_created": 8,
          "flaggo.materializer.duplicate_observations": 1,
          "flaggo.materializer.conflicts_created": 0,
          "flaggo.materializer.diagnostics_created": 0,
          "flaggo.materializer.observed_at": current,
        },
      }],
      spans: [],
      metrics: [],
      failures: [],
    },
    processes: processStates(true),
    readiness: {
      ...readyStates(),
      "flaggo-otel-ingestion": {
        status: "ready",
        body: {
          inbox: {
            retainedBatchCount: 3,
            retainedPayloadBytes: 4_096,
            oldestRetainedAt: "2025-10-09T08:53:23.000Z",
          },
          status: "ready",
        },
      },
    },
    now,
  });

  assert.deepEqual(view.evidence, {
    inboxRetainedBatches: 3,
    inboxRetainedBytes: 4_096,
    inboxOldestAgeMs: 2_000,
    checkpointBatchId: "42",
    pendingBatches: 0,
    evidenceFreshnessMs: 300,
    evidenceStoreObservations: 12,
    evidenceStoreProvenance: 12,
    batchesRead: 2,
    observationsCreated: 8,
    duplicateObservations: 1,
    conflictsCreated: 0,
    diagnosticsCreated: 0,
    observedAt: current,
    stale: false,
  });
});

test("aggregates materializer progress across committed pages", () => {
  const latest = "2025-10-09T08:53:20.123Z";
  const earlier = "2025-10-09T08:53:19.123Z";
  const view = createObserverView({
    telemetry: {
      logs: [{
        ...log(
          "latest-page",
          "flaggo-evidence-materializer",
          "flaggo.materializer.page.committed",
        ),
        observedAt: latest,
        attributes: {
          "event.name": "flaggo.materializer.page.committed",
          "otel.scope.name": "flaggo.evidence-materializer",
          "flaggo.materializer.checkpoint_batch_id": "43",
          "flaggo.materializer.pending_batch_count": 0,
          "flaggo.materializer.batches_read": 1,
          "flaggo.materializer.observations_created": 0,
          "flaggo.materializer.duplicate_observations": 0,
          "flaggo.materializer.conflicts_created": 0,
          "flaggo.materializer.diagnostics_created": 0,
          "flaggo.materializer.observed_at": latest,
        },
      }, {
        ...log(
          "evidence-page",
          "flaggo-evidence-materializer",
          "flaggo.materializer.page.committed",
        ),
        observedAt: earlier,
        attributes: {
          "event.name": "flaggo.materializer.page.committed",
          "otel.scope.name": "flaggo.evidence-materializer",
          "flaggo.materializer.checkpoint_batch_id": "42",
          "flaggo.materializer.pending_batch_count": 0,
          "flaggo.materializer.batches_read": 2,
          "flaggo.materializer.observations_created": 8,
          "flaggo.materializer.duplicate_observations": 1,
          "flaggo.materializer.conflicts_created": 0,
          "flaggo.materializer.diagnostics_created": 0,
          "flaggo.materializer.observed_at": earlier,
        },
      }],
      spans: [],
      metrics: [],
      failures: [],
    },
    processes: processStates(true),
    readiness: readyStates(),
    now,
  });

  assert.equal(view.evidence.checkpointBatchId, "43");
  assert.equal(view.evidence.batchesRead, 3);
  assert.equal(view.evidence.observationsCreated, 8);
  assert.equal(view.evidence.duplicateObservations, 1);
});

test("retains lifecycle phases when recent decisions exceed the timeline limit", () => {
  const activation = {
    id: "activation",
    observedAt: "2025-10-09T08:50:00.000Z",
    phase: "activation",
    service: "flaggo-contract-service",
    signal: "log",
    name: "flaggo.activation.completed",
    title: "Activation completed",
    attributes: {},
  };
  const decisions = Array.from({ length: 205 }, (_, index) => ({
    id: `decision-${index}`,
    observedAt: new Date(
      Date.parse("2025-10-09T08:51:00.000Z") + index * 1_000,
    ).toISOString(),
    phase: "decision",
    service: "tetris",
    signal: "application",
    name: "tetris.dropInterval.decision",
    title: `Decision ${index}`,
    attributes: {},
  }));
  const view = createObserverView({
    telemetry: {
      logs: [],
      spans: [],
      metrics: [],
      failures: [],
    },
    processes: processStates(true),
    readiness: readyStates(),
    now,
    applicationEvents: [...decisions, activation],
  });

  assert.ok(view.timeline.some((event) => event.id === "activation"));
  assert.equal(view.controlLoop.activation?.name, "flaggo.activation.completed");
});

test("reserves activity history independently of noisy decisions and not-eligible cycles", () => {
  const base = Date.parse(current);
  const cycles = Array.from({ length: 130 }, (_, index) => ({
    ...log(`cycle-${index}`, "flaggo-async-analysis", "flaggo.analysis.cycle.completed"),
    observedAt: new Date(base + index).toISOString(),
    attributes: {
      "otel.scope.name": "flaggo.async-analysis",
      "flaggo.operation.outcome": index < 65 ? "no_candidate" : "not_eligible",
    },
  }));
  const decisions = Array.from({ length: 300 }, (_, index) => ({
    id: `decision-${index}`,
    observedAt: new Date(base + 1_000 + index).toISOString(),
    phase: "decision",
    service: "tetris",
    signal: "application",
    name: "tetris.dropInterval.decision",
    title: "Decision",
    attributes: {},
  }));
  const view = createObserverView({
    telemetry: { logs: cycles, spans: [], metrics: [], failures: [] },
    processes: processStates(true),
    readiness: readyStates(),
    now,
    applicationEvents: decisions,
  });
  const activity = view.timeline.filter((event) => event.phase === "analysis");
  assert.equal(activity.filter((event) => event.outcome === "no_candidate").length, 60);
  assert.equal(activity.filter((event) => event.outcome === "not_eligible").length, 60);
  assert.ok(activity.some((event) => event.id === "cycle-64"));
  assert.ok(activity.some((event) => event.id === "cycle-129"));
  assert.ok(view.timeline.length <= 325);
});

test("derives health from process and readiness while retaining activity age", () => {
  const old = "2025-10-09T08:52:20.123Z";
  const processes = processStates(true);
  processes["flaggo-async-analysis"] = { running: false };
  const view = createObserverView({
    telemetry: {
      logs: [{
        ...log("old", "flaggo-evidence-materializer", "flaggo.service.started"),
        observedAt: old,
      }],
      spans: [],
      metrics: [],
      failures: [],
    },
    processes,
    readiness: {
      ...readyStates(),
      "flaggo-decision-service": { status: "unreachable" },
    },
    now,
  });
  const statuses = Object.fromEntries(
    view.services.map((service) => [service.name, service.status]),
  );

  assert.equal(statuses["flaggo-contract-service"], "healthy");
  assert.equal(statuses["flaggo-decision-service"], "degraded");
  assert.equal(statuses["flaggo-otel-ingestion"], "healthy");
  assert.equal(statuses["flaggo-evidence-materializer"], "healthy");
  assert.equal(statuses["flaggo-async-analysis"], "failed");
  assert.match(
    view.services.find((service) =>
      service.name === "flaggo-evidence-materializer"
    ).detail,
    /last telemetry 1m ago/u,
  );
});

test("retains bounded activity across failed, empty, and recovered polls without caching health or metrics", () => {
  const base = {
    telemetry: {
      logs: Array.from({ length: 450 }, (_, index) => ({
        ...log(`history-${index}`, "flaggo-async-analysis", "flaggo.analysis.cycle.completed"),
        observedAt: new Date(now.getTime() + index).toISOString(),
      })),
      spans: [],
      metrics: [{ name: "flaggo_decision_evaluations_total", value: 7, labels: {} }],
      failures: [],
    },
    processes: processStates(true),
    readiness: readyStates(),
    now,
  };
  const initial = createObserverView(base);
  assert.equal(initial.timeline.length, 200);
  assert.equal(initial.metrics.length, 1);
  const failed = createObserverView({
    ...base,
    previous: initial,
    telemetry: {
      logs: [], spans: [], metrics: [],
      failures: [{ source: "loki", message: "HTTP 503", observedAt: now.toISOString() }],
    },
    readiness: {},
  });
  assert.deepEqual(failed.timeline, initial.timeline);
  assert.equal(failed.activity.status, "stale");
  assert.match(failed.activity.error, /HTTP 503/);
  assert.equal(failed.activity.lastSuccessfulPollAt, now.toISOString());
  assert.deepEqual(failed.metrics, []);
  assert.equal(failed.services[0].readiness, "unreachable");
  const recovered = createObserverView({
    ...base,
    previous: failed,
    telemetry: { ...base.telemetry, logs: [base.telemetry.logs.at(-1)] },
    now: new Date(now.getTime() + 1_000),
  });
  assert.deepEqual(recovered.timeline, initial.timeline);
  assert.equal(recovered.activity.status, "current");
  assert.equal(recovered.activity.error, undefined);
  assert.equal(recovered.activity.lastSuccessfulPollAt, new Date(now.getTime() + 1_000).toISOString());
  const empty = createObserverView({
    ...base, previous: recovered,
    telemetry: { logs: [], spans: [], metrics: [], failures: [] },
  });
  assert.deepEqual(empty.timeline, initial.timeline);
  assert.equal(empty.activity.status, "current");
  const newest = createObserverView({
    ...base, previous: empty,
    telemetry: {
      ...base.telemetry,
      logs: Array.from({ length: 450 }, (_, index) => ({
        ...base.telemetry.logs[index],
        id: `new-${index}`,
        observedAt: new Date(now.getTime() + 1_000 + index).toISOString(),
      })),
    },
  });
  assert.equal(newest.timeline.length, 200);
  assert.equal(newest.timeline[0].id, "new-449");
  assert.equal(new Set(newest.timeline.map((event) => event.id)).size, 200);
});

function log(id, service, eventName) {
  const scopes = {
    "flaggo-contract-service": "flaggo.contract-service",
    "flaggo-decision-service": "flaggo.decision-service",
    "flaggo-otel-ingestion": "flaggo.otel-ingestion",
    "flaggo-evidence-materializer": "flaggo.evidence-materializer",
    "flaggo-async-analysis": "flaggo.async-analysis",
  };
  return {
    id,
    observedAt: current,
    service,
    eventName,
    message: eventName,
    attributes: {
      "event.name": eventName,
      "otel.scope.name": scopes[service],
    },
  };
}

function processStates(running) {
  return Object.fromEntries(
    serviceNames.map((name) => [name, { running }]),
  );
}

function readyStates() {
  return {
    "flaggo-contract-service": { status: "ready" },
    "flaggo-decision-service": { status: "ready" },
    "flaggo-otel-ingestion": { status: "ready" },
  };
}
