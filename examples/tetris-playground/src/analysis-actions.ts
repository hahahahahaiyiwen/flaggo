import type { TelemetryLog } from "./backend.js";
import type {
  AnalysisAction,
  AnalysisActionObservation,
  AnalysisActionsView,
  AnalysisSelection,
} from "./types.js";

export const actionLogNames = [
  "flaggo.analysis.action.started",
  "flaggo.analysis.action.completed",
] as const;
export const actionObservationLimit = 400;
export const actionLimit = 200;
const kinds = new Set([
  "session.start", "session.run", "provider.request", "evidence.describe",
  "evidence.cutoff", "evidence.sql", "artifact.read", "artifact.write",
  "artifact.append", "proposal.submit",
]);
const outcomes = new Set([
  "success", "failed", "timeout", "cancelled", "interrupted", "superseded",
  "handoff", "no_change", "no_candidate",
]);
const digest = /^sha256:[a-f0-9]{64}$/u;
const identity = /^[A-Za-z0-9_.:-]{1,128}$/u;
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/iu;
const rfc3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/u;
export const actionAttributeNames = [
  "flaggo.analysis.action.id", "flaggo.analysis.action.kind",
  "flaggo.analysis.action.outcome", "flaggo.analysis.action.duration_ms",
  "flaggo.analysis.query.digest", "flaggo.analysis.snapshot.cutoff",
  "flaggo.analysis.snapshot.watermark", "flaggo.analysis.result.rows",
  "flaggo.analysis.result.bytes", "flaggo.analysis.artifact.digest",
  "flaggo.analysis.tool_call.id", "flaggo.analysis.session.id",
] as const;

export function validAnalysisSelection(value: AnalysisSelection): boolean {
  return digest.test(value.contractDigest)
    && identity.test(value.cycleId) && identity.test(value.attemptId);
}

export function matchesAnalysisSelection(
  value: AnalysisSelection,
  selection: AnalysisSelection,
): boolean {
  return value.contractDigest === selection.contractDigest
    && value.cycleId === selection.cycleId && value.attemptId === selection.attemptId;
}

export function actionObservation(log: TelemetryLog): AnalysisActionObservation | undefined {
  if (log.service !== "flaggo-async-analysis"
    || log.attributes["otel.scope.name"] !== "flaggo.async-analysis"
    || !actionLogNames.some((name) => name === log.eventName)
    || !validTimestamp(log.observedAt)) return undefined;
  const attrs = log.attributes;
  const contractDigest = attrs["flaggo.contract.digest"];
  const contractName = attrs["flaggo.contract.name"];
  const cycleId = attrs["flaggo.analysis.cycle.id"];
  const attemptId = attrs["flaggo.analysis.attempt.id"];
  const actionId = attrs["flaggo.analysis.action.id"];
  const kind = attrs["flaggo.analysis.action.kind"];
  const outcome = attrs["flaggo.analysis.action.outcome"];
  if (typeof contractDigest !== "string" || typeof cycleId !== "string"
    || typeof attemptId !== "string" || typeof contractName !== "string"
    || !identity.test(contractName)
    || !validAnalysisSelection({ contractDigest, cycleId, attemptId })
    || typeof actionId !== "string" || !uuid.test(actionId)
    || typeof kind !== "string" || !kinds.has(kind)
    || typeof outcome !== "string") return undefined;
  const event = log.eventName === actionLogNames[0] ? "started" : "completed";
  const duration = nonnegativeInteger(attrs["flaggo.analysis.action.duration_ms"]);
  if (event === "started" ? outcome !== "running" : !outcomes.has(outcome) || duration === undefined) {
    return undefined;
  }
  const attributes: Record<string, string | number> = {};
  if (event === "completed" && duration !== undefined) {
    attributes["flaggo.analysis.action.duration_ms"] = duration;
  }
  for (const name of ["flaggo.analysis.query.digest", "flaggo.analysis.artifact.digest",
    "flaggo.analysis.session.id"]) {
    const value = attrs[name];
    if (typeof value === "string" && digest.test(value)) attributes[name] = value;
  }
  for (const name of ["flaggo.analysis.snapshot.watermark", "flaggo.analysis.result.rows",
    "flaggo.analysis.result.bytes"]) {
    const value = nonnegativeInteger(attrs[name]);
    if (value !== undefined) attributes[name] = value;
  }
  const cutoff = attrs["flaggo.analysis.snapshot.cutoff"];
  if (typeof cutoff === "string" && validTimestamp(cutoff)) {
    attributes["flaggo.analysis.snapshot.cutoff"] = cutoff;
  }
  const toolCall = attrs["flaggo.analysis.tool_call.id"];
  if (typeof toolCall === "string" && /^[\x21-\x7e]{1,128}$/u.test(toolCall)) {
    attributes["flaggo.analysis.tool_call.id"] = toolCall;
  }
  const category = attrs["flaggo.failure.category"];
  if (typeof category === "string" && /^[A-Za-z0-9_.:-]{1,64}$/u.test(category)) {
    attributes["flaggo.failure.category"] = category;
  }
  return {
    contractDigest, contractName, cycleId, attemptId, actionId, kind, event,
    observedAt: log.observedAt, outcome, attributes,
    ...(log.traceId !== undefined && /^[a-f0-9]{32}$/iu.test(log.traceId)
      ? { traceId: log.traceId } : {}),
  };
}

export function createAnalysisActionsView(input: {
  readonly logs: readonly TelemetryLog[];
  readonly observations?: readonly AnalysisActionObservation[];
  readonly now: Date;
  readonly previous?: AnalysisActionsView;
  readonly error?: string;
  readonly queryLimited?: boolean;
  readonly selection?: AnalysisSelection;
}): AnalysisActionsView {
  const unique = new Map<string, AnalysisActionObservation>();
  for (const value of [
    ...(input.previous?.observations ?? []),
    ...(input.observations ?? []),
    ...input.logs.flatMap((log) => {
      const value = actionObservation(log);
      return value === undefined ? [] : [value];
    }),
  ]) {
    if (input.selection !== undefined && !matchesAnalysisSelection(value, input.selection)) continue;
    unique.set(JSON.stringify(value), value);
  }
  const ordered = [...unique.values()].sort((left, right) =>
    Date.parse(right.observedAt) - Date.parse(left.observedAt));
  const observations = ordered.slice(0, actionObservationLimit);
  const groups = new Map<string, AnalysisActionObservation[]>();
  for (const observation of observations) {
    const key = JSON.stringify([observation.contractDigest, observation.cycleId,
      observation.attemptId, observation.actionId]);
    const group = groups.get(key) ?? [];
    group.push(observation);
    groups.set(key, group);
  }
  const conflictingKeys = new Set(input.previous?.actions
    .filter((action) => action.status === "unknown/conflicting").map((action) => action.key));
  const allActions = [...groups].map(([key, values]) =>
    reconstructAction(key, values, conflictingKeys.has(key)));
  return {
    status: input.error === undefined ? "current" : "stale",
    ...(input.error === undefined
      ? { lastSuccessfulPollAt: input.now.toISOString() }
      : input.previous?.lastSuccessfulPollAt === undefined
        ? {} : { lastSuccessfulPollAt: input.previous.lastSuccessfulPollAt }),
    ...(input.error === undefined ? {} : { error: input.error }),
    observations,
    actions: allActions.slice(0, actionLimit),
    limited: input.queryLimited === true || input.previous?.limited === true
      || ordered.length > actionObservationLimit || allActions.length > actionLimit,
    observationLimit: actionObservationLimit,
    actionLimit,
  };
}

function reconstructAction(
  key: string,
  values: readonly AnalysisActionObservation[],
  knownConflict: boolean,
): AnalysisAction {
  const latest = values[0]!;
  const starts = values.filter((value) => value.event === "started");
  const terminals = values.filter((value) => value.event === "completed");
  const terminal = terminals[0];
  const contradictory = knownConflict || new Set(values.map((value) => value.kind)).size > 1
    || new Set(terminals.map((value) => JSON.stringify([
      value.outcome, value.contractName, value.attributes,
    ]))).size > 1;
  return {
    key, contractDigest: latest.contractDigest, contractName: latest.contractName,
    cycleId: latest.cycleId, attemptId: latest.attemptId, actionId: latest.actionId,
    kind: contradictory ? "unknown/conflicting" : latest.kind,
    latestObservedAt: latest.observedAt,
    ...(starts.length === 0 ? {} : { startedAt: starts.at(-1)!.observedAt }),
    ...(terminal === undefined || contradictory ? {} : { completedAt: terminal.observedAt }),
    status: contradictory ? "unknown/conflicting"
      : terminal === undefined ? "completion not observed" : "completed",
    ...(terminal === undefined || contradictory ? {} : { outcome: terminal.outcome }),
    attributes: contradictory ? {} : Object.assign({}, ...[...values].reverse().map((value) => value.attributes)),
    ...(latest.traceId === undefined ? {} : { traceId: latest.traceId }),
  };
}

function validTimestamp(value: string): boolean {
  return value.length <= 64 && rfc3339.test(value) && Number.isFinite(Date.parse(value));
}

function nonnegativeInteger(value: unknown): number | undefined {
  const number = typeof value === "number" ? value
    : typeof value === "string" && /^\d{1,16}$/u.test(value) ? Number(value) : NaN;
  return Number.isSafeInteger(number) && number >= 0 ? number : undefined;
}
