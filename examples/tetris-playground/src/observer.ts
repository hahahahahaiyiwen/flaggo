import type {
  TelemetryLog,
  TelemetryQueryResult,
  TelemetrySpan,
} from "./backend.js";
import type {
  ActivityFeedStatus,
  AnalysisActionsView,
  ClosedLoopEvent,
  ClosedLoopPhase,
  ControlLoopMilestone,
  ControlLoopProgress,
  EvidenceProgress,
  MetricView,
  ProcessState,
  ReadinessState,
  ServiceStatus,
} from "./types.js";
import { createAnalysisActionsView } from "./analysis-actions.js";

export interface ObserverInput {
  readonly telemetry: TelemetryQueryResult;
  readonly processes: Readonly<Record<string, ProcessState>>;
  readonly readiness: Readonly<Record<string, ReadinessState>>;
  readonly now: Date;
  readonly applicationEvents?: readonly ClosedLoopEvent[];
  readonly previous?: ObserverView;
}

export interface ObserverView {
  readonly services: readonly ServiceStatus[];
  readonly evidence: EvidenceProgress;
  readonly controlLoop: ControlLoopProgress;
  readonly metrics: readonly MetricView[];
  readonly timeline: readonly ClosedLoopEvent[];
  readonly activity: ActivityFeedStatus;
  readonly analysisActions: AnalysisActionsView;
}

const staleAfterMilliseconds = 15_000;

const services = [
  {
    name: "flaggo-contract-service",
    displayName: "Contract Service",
    readiness: true,
  },
  {
    name: "flaggo-decision-service",
    displayName: "Decision Service",
    readiness: true,
  },
  {
    name: "flaggo-otel-ingestion",
    displayName: "OTel Ingestion",
    readiness: true,
  },
  {
    name: "flaggo-evidence-materializer",
    displayName: "Evidence Materializer",
    readiness: false,
  },
  {
    name: "flaggo-async-analysis",
    displayName: "Async Analysis",
    readiness: false,
  },
] as const;

const serviceScopes = new Map<string, string>([
  ["flaggo-contract-service", "flaggo.contract-service"],
  ["flaggo-decision-service", "flaggo.decision-service"],
  ["flaggo-otel-ingestion", "flaggo.otel-ingestion"],
  ["flaggo-evidence-materializer", "flaggo.evidence-materializer"],
  ["flaggo-async-analysis", "flaggo.async-analysis"],
]);

const allFlaggoServices = new Set(serviceScopes.keys());

const logPhases = new Map<string, {
  readonly phase: ClosedLoopPhase;
  readonly services: ReadonlySet<string>;
}>([
  ["flaggo.service.started", {
    phase: "lifecycle",
    services: allFlaggoServices,
  }],
  ["flaggo.service.stopping", {
    phase: "lifecycle",
    services: allFlaggoServices,
  }],
  ["flaggo.service.stopped", {
    phase: "lifecycle",
    services: allFlaggoServices,
  }],
  ["flaggo.service.failed", {
    phase: "lifecycle",
    services: allFlaggoServices,
  }],
  ["flaggo.otlp.request.rejected", {
    phase: "ingestion",
    services: new Set(["flaggo-otel-ingestion"]),
  }],
  ["flaggo.inbox.retention.completed", {
    phase: "ingestion",
    services: new Set(["flaggo-otel-ingestion"]),
  }],
  ["flaggo.materializer.page.committed", {
    phase: "materialization",
    services: new Set(["flaggo-evidence-materializer"]),
  }],
  ["flaggo.materializer.catalog.activated", {
    phase: "materialization",
    services: new Set(["flaggo-evidence-materializer"]),
  }],
  ["flaggo.materializer.catalog.refresh_failed", {
    phase: "materialization",
    services: new Set(["flaggo-evidence-materializer"]),
  }],
  ["flaggo.analysis.cycle.completed", {
    phase: "analysis",
    services: new Set(["flaggo-async-analysis"]),
  }],
  ["flaggo.analysis.cycle.failed", {
    phase: "analysis",
    services: new Set(["flaggo-async-analysis"]),
  }],
  ["flaggo.candidate.admitted", {
    phase: "candidate",
    services: new Set(["flaggo-contract-service"]),
  }],
  ["flaggo.candidate.rejected", {
    phase: "candidate",
    services: new Set(["flaggo-contract-service"]),
  }],
  ["flaggo.activation.completed", {
    phase: "activation",
    services: new Set(["flaggo-contract-service"]),
  }],
  ["flaggo.activation.rejected", {
    phase: "activation",
    services: new Set(["flaggo-contract-service"]),
  }],
  ["flaggo.activation.resolved", {
    phase: "activation",
    services: new Set(["flaggo-contract-service"]),
  }],
  ["flaggo.activation.scan_failed", {
    phase: "activation",
    services: new Set(["flaggo-contract-service"]),
  }],
]);

export const activityLogNames = [...logPhases]
  .filter(([, registration]) => isActivityPhase(registration.phase))
  .map(([name]) => name);

const spanPhases = new Map<string, {
  readonly phase: ClosedLoopPhase;
  readonly service: string;
}>([
  ["flaggo.decision.evaluate", {
    phase: "decision",
    service: "flaggo-decision-service",
  }],
  ["flaggo.otlp.append", {
    phase: "ingestion",
    service: "flaggo-otel-ingestion",
  }],
]);

const registeredAttributeNames = new Set([
  "service.name",
  "service.namespace",
  "service.version",
  "service.instance.id",
  "deployment.environment.name",
  "flaggo.request.correlation_id",
  "flaggo.contract.name",
  "flaggo.contract.digest",
  "flaggo.executable.digest",
  "flaggo.candidate.digest",
  "flaggo.activation.superseded_count",
  "flaggo.batch.id",
  "flaggo.analysis.cycle.id",
  "flaggo.analysis.attempt.id",
  "flaggo.operation.name",
  "flaggo.operation.outcome",
  "flaggo.failure.category",
  "flaggo.evaluation.source",
  "flaggo.otlp.signal",
  "flaggo.otlp.encoding",
  "flaggo.otlp.compression",
  "flaggo.materializer.item.outcome",
  "flaggo.inbox.expired.batch_count",
  "flaggo.inbox.expired.payload_bytes",
  "flaggo.materializer.active_route_count",
  "flaggo.materializer.current_contract_count",
  "flaggo.materializer.evidence.conflict_count",
  "flaggo.materializer.evidence.diagnostic_count",
  "flaggo.materializer.evidence.has_cached_catalog",
  "flaggo.materializer.evidence.observation_count",
  "flaggo.materializer.evidence.provenance_count",
  "flaggo.materializer.checkpoint_batch_id",
  "flaggo.materializer.pending_batch_count",
  "flaggo.materializer.observed_at",
  "flaggo.materializer.oldest_pending_received_at",
  "flaggo.materializer.newest_pending_received_at",
  "flaggo.materializer.oldest_pending_age_ms",
  "flaggo.materializer.newest_evidence_observed_at_unix_nano",
  "flaggo.materializer.evidence_freshness_ms",
  "flaggo.materializer.batches_read",
  "flaggo.materializer.conflicts_created",
  "flaggo.materializer.diagnostics_created",
  "flaggo.materializer.duplicate_observations",
  "flaggo.materializer.observations_created",
  "flaggo.materializer.provenance_created",
  "flaggo.materializer.catalog_retained",
]);

const registeredMetricNames = [
  "flaggo_contract_operations",
  "flaggo_contract_operation_duration",
  "flaggo_decision_evaluations",
  "flaggo_decision_evaluation_duration",
  "flaggo_otlp_requests",
  "flaggo_otlp_request_size",
  "flaggo_inbox_retained_batches",
  "flaggo_inbox_retained_bytes",
  "flaggo_inbox_oldest_age",
  "flaggo_inbox_expired_batches",
  "flaggo_inbox_expired_bytes",
  "flaggo_materializer_pages",
  "flaggo_materializer_page_duration",
  "flaggo_materializer_pending_batches",
  "flaggo_materializer_oldest_pending_age",
  "flaggo_materializer_evidence_freshness",
  "flaggo_materializer_items",
  "flaggo_analysis_cycles",
  "flaggo_analysis_cycle_duration",
  "flaggo_analysis_workers_active",
  "flaggo_candidate_submissions",
  "flaggo_activation_attempts",
] as const;

const metricLabelNames = new Map<string, string>(
  [...registeredAttributeNames].map((name) => [
    name.replaceAll(".", "_"),
    name,
  ]),
);

export function createObserverView(input: ObserverInput): ObserverView {
  const events = [
    ...(input.applicationEvents ?? []),
    ...input.telemetry.logs.flatMap(logEvent),
    ...input.telemetry.spans.flatMap(spanEvent),
    ...(input.previous?.timeline.filter((event) => isActivityPhase(event.phase)) ?? []),
  ]
    .sort((left, right) => Date.parse(right.observedAt) - Date.parse(left.observedAt))
    .filter(dedupeById());
  const activityFailures = input.telemetry.failures.filter((failure) =>
    failure.source === "loki" || failure.source === "tempo"
  );
  const lastSuccessfulPollAt = input.previous?.activity.lastSuccessfulPollAt;
  return {
    services: services.map((service) => serviceStatus(service, input)),
    evidence: evidenceProgress(input),
    controlLoop: controlLoopProgress(events),
    metrics: input.telemetry.metrics.flatMap(registeredMetric),
    timeline: selectTimelineEvents(events, 200),
    activity: activityFailures.length === 0
      ? { status: "current", lastSuccessfulPollAt: input.now.toISOString() }
      : {
        status: "stale",
        ...(lastSuccessfulPollAt === undefined ? {} : { lastSuccessfulPollAt }),
        error: activityFailures.map((failure) =>
          `${failure.source}: ${failure.message}`
        ).join("; "),
      },
    analysisActions: createAnalysisActionsView({
      logs: input.telemetry.logs,
      now: input.now,
      ...(input.previous === undefined ? {} : { previous: input.previous.analysisActions }),
      ...(input.telemetry.failures.some((failure) => failure.source === "loki")
        ? { error: "Loki action telemetry unavailable; retaining latest observed facts." } : {}),
      queryLimited: input.telemetry.actionQueryLimited === true,
    }),
  };
}

export function selectTimelineEvents(
  values: readonly ClosedLoopEvent[],
  recentLimit: number,
): readonly ClosedLoopEvent[] {
  const ordered = [...values].sort((left, right) =>
    Date.parse(right.observedAt) - Date.parse(left.observedAt)
  );
  const activity = ordered.filter((event) => isActivityPhase(event.phase));
  const selected = [
    ...ordered.slice(0, recentLimit),
    ...activity.slice(0, 60),
    ...activity.filter((event) =>
      event.name !== "flaggo.analysis.cycle.completed"
      || (event.outcome ?? event.attributes["flaggo.operation.outcome"]) !== "not_eligible"
    ).slice(0, 60),
  ].filter(dedupeById());
  const includedPhases = new Set(selected.map((event) => event.phase));
  for (const event of ordered) {
    if (includedPhases.has(event.phase)) continue;
    selected.push(event);
    includedPhases.add(event.phase);
  }
  return selected.sort((left, right) =>
    Date.parse(right.observedAt) - Date.parse(left.observedAt)
  );
}

function isActivityPhase(phase: ClosedLoopPhase): boolean {
  return phase === "analysis" || phase === "candidate" || phase === "activation";
}

function controlLoopProgress(
  timeline: readonly ClosedLoopEvent[],
): ControlLoopProgress {
  return {
    ...latestMilestone(timeline, "analysis", "analysis"),
    ...latestMilestone(timeline, "candidate", "candidate"),
    ...latestMilestone(timeline, "activation", "activation"),
  };
}

function latestMilestone<TKey extends keyof ControlLoopProgress>(
  timeline: readonly ClosedLoopEvent[],
  phase: ClosedLoopPhase,
  key: TKey,
): { readonly [name in TKey]?: ControlLoopMilestone } {
  const event = timeline.find((candidate) => candidate.phase === phase);
  if (event === undefined) return {};
  const outcome = event.outcome;
  const failed = event.name.endsWith(".failed")
    || event.name.endsWith(".rejected")
    || outcome === "failure"
    || outcome === "invalid"
    || outcome === "unavailable";
  return {
    [key]: {
      observedAt: event.observedAt,
      name: event.name,
      ...(outcome === undefined ? {} : { outcome }),
      status: failed ? "failure" : "success",
      ...milestoneText(event, "flaggo.contract.digest", "contractDigest"),
      ...milestoneText(event, "flaggo.analysis.cycle.id", "cycleId"),
      ...milestoneText(event, "flaggo.candidate.digest", "candidateDigest"),
      ...milestoneText(event, "flaggo.executable.digest", "executableDigest"),
    },
  } as { readonly [name in TKey]?: ControlLoopMilestone };
}

function milestoneText<TKey extends string>(
  event: ClosedLoopEvent,
  attribute: string,
  key: TKey,
): { readonly [name in TKey]?: string } {
  const value = event.attributes[attribute];
  return typeof value === "string" && value !== ""
    ? { [key]: value } as { readonly [name in TKey]?: string }
    : {};
}

function serviceStatus(
  definition: (typeof services)[number],
  input: ObserverInput,
): ServiceStatus {
  const process = input.processes[definition.name];
  const readiness = input.readiness[definition.name] ?? {
    status: definition.readiness ? "unreachable" : "not-applicable",
  };
  const signals = [
    ...input.telemetry.logs.filter((entry) => entry.service === definition.name),
    ...input.telemetry.spans.filter((entry) => entry.service === definition.name),
  ].sort((left, right) =>
    Date.parse(right.observedAt) - Date.parse(left.observedAt)
  );
  const latest = signals[0];
  const latestLifecycleFailure = input.telemetry.logs
    .filter((entry) =>
      entry.service === definition.name
      && entry.eventName === "flaggo.service.failed"
    )
    .sort((left, right) =>
      Date.parse(right.observedAt) - Date.parse(left.observedAt)
    )[0];
  const latestEventName = latest !== undefined && "eventName" in latest
    ? latest.eventName
    : undefined;
  const running = process?.running === true;
  let status: ServiceStatus["status"];
  let detail: string;
  if (!running) {
    status = latestEventName === "flaggo.service.stopped" ? "stopped" : "failed";
    detail = latestEventName === "flaggo.service.stopped"
      ? "process stopped"
      : "process exited";
  } else if (
    latestLifecycleFailure !== undefined
    && latestLifecycleFailure.id === latest?.id
  ) {
    status = "failed";
    detail = latestLifecycleFailure.eventName ?? "service failure";
  } else if (readiness.status === "not-ready" || readiness.status === "unreachable") {
    status = "degraded";
    detail = `readiness ${readiness.status}`;
  } else {
    status = "healthy";
    const activity = latest === undefined
      ? "no telemetry observed yet"
      : `last telemetry ${formatAge(
        input.now.getTime() - Date.parse(latest.observedAt),
      )} ago`;
    detail = readiness.status === "ready"
      ? `ready; ${activity}`
      : `process running; ${activity}`;
  }
  return {
    name: definition.name,
    displayName: definition.displayName,
    process: running ? "running" : "exited",
    readiness: readiness.status,
    status,
    detail,
    ...(latest === undefined ? {} : { lastSignalAt: latest.observedAt }),
  };
}

function evidenceProgress(input: ObserverInput): EvidenceProgress {
  const pages = input.telemetry.logs
    .filter((entry) => entry.eventName === "flaggo.materializer.page.committed")
    .sort((left, right) =>
      Date.parse(right.observedAt) - Date.parse(left.observedAt)
    );
  const latest = pages[0];
  const ingestionBody = objectValue(
    input.readiness["flaggo-otel-ingestion"]?.body,
  );
  const inbox = objectValue(ingestionBody?.inbox);
  const attributes = latest?.attributes ?? {};
  const observedAt = stringAttribute(
    attributes,
    "flaggo.materializer.observed_at",
  ) ?? latest?.observedAt;
  const oldestRetainedAt = typeof inbox?.oldestRetainedAt === "string"
    ? inbox.oldestRetainedAt
    : undefined;
  const oldestRetainedTimestamp = oldestRetainedAt === undefined
    ? Number.NaN
    : Date.parse(oldestRetainedAt);
  return {
    ...numberProperty(inbox, "retainedBatchCount", "inboxRetainedBatches"),
    ...numberProperty(inbox, "retainedPayloadBytes", "inboxRetainedBytes"),
    ...(Number.isFinite(oldestRetainedTimestamp)
      ? {
        inboxOldestAgeMs: Math.max(
          0,
          input.now.getTime() - oldestRetainedTimestamp,
        ),
      }
      : {}),
    ...stringMetric(
      attributes,
      "flaggo.materializer.checkpoint_batch_id",
      "checkpointBatchId",
    ),
    ...numberMetric(
      attributes,
      "flaggo.materializer.pending_batch_count",
      "pendingBatches",
    ),
    ...numberMetric(
      attributes,
      "flaggo.materializer.evidence_freshness_ms",
      "evidenceFreshnessMs",
    ),
    ...numberMetric(
      attributes,
      "flaggo.materializer.evidence.observation_count",
      "evidenceStoreObservations",
    ),
    ...numberMetric(
      attributes,
      "flaggo.materializer.evidence.provenance_count",
      "evidenceStoreProvenance",
    ),
    ...summedLogMetric(
      pages,
      "flaggo.materializer.batches_read",
      "batchesRead",
    ),
    ...summedLogMetric(
      pages,
      "flaggo.materializer.observations_created",
      "observationsCreated",
    ),
    ...summedLogMetric(
      pages,
      "flaggo.materializer.duplicate_observations",
      "duplicateObservations",
    ),
    ...summedLogMetric(
      pages,
      "flaggo.materializer.conflicts_created",
      "conflictsCreated",
    ),
    ...summedLogMetric(
      pages,
      "flaggo.materializer.diagnostics_created",
      "diagnosticsCreated",
    ),
    ...(observedAt === undefined ? {} : { observedAt }),
    stale: observedAt === undefined
      || input.now.getTime() - Date.parse(observedAt) > staleAfterMilliseconds,
  };
}

function formatAge(milliseconds: number): string {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.round(seconds / 60);
  return `${minutes}m`;
}

function logEvent(log: TelemetryLog): readonly ClosedLoopEvent[] {
  if (log.eventName === undefined) return [];
  const registration = logPhases.get(log.eventName);
  if (
    registration === undefined
    || !registration.services.has(log.service)
    || !hasRegisteredScope(log)
  ) {
    return [];
  }
  const attributes = registeredAttributes(log.attributes);
  return [{
    id: log.id,
    observedAt: log.observedAt,
    phase: registration.phase,
    service: log.service,
    signal: "log",
    name: log.eventName,
    title: eventTitle(log.eventName, log.attributes),
    ...optionalText(attributes, "flaggo.operation.outcome", "outcome"),
    ...(log.severity === undefined ? {} : { severity: log.severity }),
    ...(log.traceId === undefined ? {} : { traceId: log.traceId }),
    ...(log.spanId === undefined ? {} : { spanId: log.spanId }),
    attributes,
  }];
}

function spanEvent(span: TelemetrySpan): readonly ClosedLoopEvent[] {
  const registration = spanPhases.get(span.name);
  if (
    registration === undefined
    || registration.service !== span.service
    || !hasRegisteredScope(span)
  ) {
    return [];
  }
  const attributes = registeredAttributes(span.attributes);
  return [{
    id: span.id,
    observedAt: span.observedAt,
    phase: registration.phase,
    service: span.service,
    signal: "span",
    name: span.name,
    title: eventTitle(span.name, span.attributes),
    ...optionalText(attributes, "flaggo.operation.outcome", "outcome"),
    ...(span.durationMs === undefined ? {} : { durationMs: span.durationMs }),
    traceId: span.traceId,
    ...(span.spanId === undefined ? {} : { spanId: span.spanId }),
    attributes,
  }];
}

function hasRegisteredScope(
  signal: TelemetryLog | TelemetrySpan,
): boolean {
  const expected = serviceScopes.get(signal.service);
  if (expected === undefined) return false;
  const actual = signal.attributes["otel.scope.name"];
  return actual === undefined || actual === expected;
}

function registeredAttributes(
  attributes: Readonly<Record<string, string | number | boolean>>,
): Readonly<Record<string, string | number | boolean>> {
  return Object.fromEntries(Object.entries(attributes).filter(([name]) =>
    registeredAttributeNames.has(name)
  ));
}

function registeredMetric(metric: MetricView): readonly MetricView[] {
  if (!isRegisteredMetricName(metric.name)) return [];
  const labels = Object.fromEntries(Object.entries(metric.labels).flatMap(
    ([rawName, value]) => {
      const name = metricLabelNames.get(rawName);
      return name === undefined ? [] : [[name, value]];
    },
  ));
  return [{
    ...metric,
    labels,
  }];
}

function isRegisteredMetricName(name: string): boolean {
  return registeredMetricNames.some((base) => {
    if (!name.startsWith(base)) return false;
    const suffix = name.slice(base.length);
    return suffix === "" || /^_[a-z0-9]+(?:_[a-z0-9]+)*$/u.test(suffix);
  });
}

function eventTitle(
  name: string,
  attributes: Readonly<Record<string, string | number | boolean>>,
): string {
  const outcome = stringAttribute(attributes, "flaggo.operation.outcome");
  const contract = stringAttribute(attributes, "flaggo.contract.name");
  const suffix = outcome === undefined ? "" : `: ${outcome}`;
  if (name === "flaggo.decision.evaluate") {
    return `Decision evaluated${contract === undefined ? "" : ` for ${contract}`}${suffix}`;
  }
  return `${name.replace(/^flaggo\./u, "").replaceAll(".", " ")}${suffix}`;
}

function optionalText<
  TKey extends string,
>(
  attributes: Readonly<Record<string, string | number | boolean>>,
  source: string,
  target: TKey,
): { readonly [key in TKey]?: string } {
  const value = stringAttribute(attributes, source);
  return value === undefined ? {} : { [target]: value } as {
    readonly [key in TKey]?: string;
  };
}

function stringAttribute(
  attributes: Readonly<Record<string, string | number | boolean>>,
  name: string,
): string | undefined {
  const value = attributes[name];
  return typeof value === "string" && value !== "" ? value : undefined;
}

function numberMetric<TKey extends string>(
  attributes: Readonly<Record<string, string | number | boolean>>,
  source: string,
  target: TKey,
): { readonly [key in TKey]?: number } {
  const value = attributes[source];
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isFinite(parsed)
    ? { [target]: parsed } as { readonly [key in TKey]?: number }
    : {};
}

function summedLogMetric<TKey extends string>(
  entries: readonly TelemetryLog[],
  source: string,
  target: TKey,
): { readonly [key in TKey]?: number } {
  let found = false;
  let total = 0;
  for (const entry of entries) {
    const value = entry.attributes[source];
    const parsed = typeof value === "number" ? value : Number(value);
    if (!Number.isFinite(parsed)) continue;
    found = true;
    total += parsed;
  }
  return found
    ? { [target]: total } as { readonly [key in TKey]?: number }
    : {};
}

function stringMetric<TKey extends string>(
  attributes: Readonly<Record<string, string | number | boolean>>,
  source: string,
  target: TKey,
): { readonly [key in TKey]?: string } {
  const value = attributes[source];
  return typeof value === "string" && value !== ""
    ? { [target]: value } as { readonly [key in TKey]?: string }
    : {};
}

function numberProperty<TKey extends string>(
  value: Record<string, unknown> | undefined,
  source: string,
  target: TKey,
): { readonly [key in TKey]?: number } {
  const item = value?.[source];
  return typeof item === "number" && Number.isFinite(item)
    ? { [target]: item } as { readonly [key in TKey]?: number }
    : {};
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function dedupeById(): (value: ClosedLoopEvent) => boolean {
  const seen = new Set<string>();
  return (value) => {
    if (seen.has(value.id)) return false;
    seen.add(value.id);
    return true;
  };
}
