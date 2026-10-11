import type {
  BackendFailure,
  MetricView,
} from "./types.js";
import { activityLogNames } from "./observer.js";

export interface TelemetryLog {
  readonly id: string;
  readonly observedAt: string;
  readonly service: string;
  readonly eventName?: string;
  readonly severity?: string;
  readonly message: string;
  readonly traceId?: string;
  readonly spanId?: string;
  readonly attributes: Readonly<Record<string, string | number | boolean>>;
}

export interface TelemetrySpan {
  readonly id: string;
  readonly observedAt: string;
  readonly service: string;
  readonly name: string;
  readonly durationMs?: number;
  readonly traceId: string;
  readonly spanId?: string;
  readonly attributes: Readonly<Record<string, string | number | boolean>>;
}

export interface TelemetryQueryResult {
  readonly logs: readonly TelemetryLog[];
  readonly spans: readonly TelemetrySpan[];
  readonly metrics: readonly MetricView[];
  readonly failures: readonly BackendFailure[];
}

export interface LgtmEndpoints {
  readonly lokiUrl: string;
  readonly prometheusUrl: string;
  readonly tempoUrl: string;
}

const knownAttributeNames = [
  "event.name",
  "service.name",
  "service.namespace",
  "service.version",
  "service.instance.id",
  "otel.scope.name",
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
  "flaggo.materializer.batches_read",
  "flaggo.materializer.checkpoint_batch_id",
  "flaggo.materializer.conflicts_created",
  "flaggo.materializer.diagnostics_created",
  "flaggo.materializer.duplicate_observations",
  "flaggo.materializer.evidence_freshness_ms",
  "flaggo.materializer.observations_created",
  "flaggo.materializer.observed_at",
  "flaggo.materializer.pending_batch_count",
  "flaggo.materializer.oldest_pending_received_at",
  "flaggo.materializer.newest_pending_received_at",
  "flaggo.materializer.oldest_pending_age_ms",
  "flaggo.materializer.newest_evidence_observed_at_unix_nano",
  "flaggo.materializer.provenance_created",
  "flaggo.materializer.catalog_retained",
] as const;

const normalizedAttributeNames = new Map<string, string>(
  knownAttributeNames.flatMap((name) => [
    [name, name],
    [name.replaceAll(".", "_"), name],
  ]),
);
normalizedAttributeNames.set("scope_name", "otel.scope.name");
normalizedAttributeNames.set("instrumentation_scope_name", "otel.scope.name");

export class LgtmClient {
  constructor(
    private readonly endpoints: LgtmEndpoints,
    private readonly fetchImpl: typeof fetch = fetch,
  ) {}

  async query(
    since: Date,
    signal?: AbortSignal,
  ): Promise<TelemetryQueryResult> {
    const operations = await Promise.allSettled([
      this.queryLogs(since, signal),
      this.querySpans(since, signal),
      this.queryMetrics(signal),
    ]);
    const failures: BackendFailure[] = [];
    const observedAt = new Date().toISOString();
    const sourceNames = ["loki", "tempo", "prometheus"] as const;
    for (let index = 0; index < operations.length; index += 1) {
      const operation = operations[index]!;
      if (operation.status === "rejected") {
        failures.push({
          source: sourceNames[index]!,
          message: errorMessage(operation.reason),
          observedAt,
        });
      }
    }
    const logResult = settledValue(operations[0], { logs: [], failures: [] });
    return {
      logs: logResult.logs,
      spans: settledValue(operations[1], []),
      metrics: settledValue(operations[2], []),
      failures: [...logResult.failures, ...failures],
    };
  }

  private async queryLogs(
    since: Date,
    signal?: AbortSignal,
  ): Promise<{
    readonly logs: readonly TelemetryLog[];
    readonly failures: readonly BackendFailure[];
  }> {
    const query = '{service_name=~"flaggo-.+"}';
    const activityQuery = '{service_name=~"flaggo-(async-analysis|contract-service)"} | event_name=~'
      + JSON.stringify(activityLogNames.map((name) => name.replaceAll(".", "\\.")).join("|"));
    const endNanoseconds = BigInt(Date.now()) * 1_000_000n;
    const startNanoseconds = BigInt(since.getTime()) * 1_000_000n;
    const responses = await Promise.allSettled(
      [
        { query, direction: "forward", limit: 2_000 },
        { query, direction: "backward", limit: 2_000 },
        { query: activityQuery, direction: "backward", limit: 60 },
        {
          query: activityQuery + ' | flaggo_operation_outcome != "not_eligible"',
          direction: "backward",
          limit: 60,
        },
      ].map(async (request) => {
        const url = new URL(
          "/loki/api/v1/query_range",
          this.endpoints.lokiUrl,
        );
        url.searchParams.set("query", request.query);
        url.searchParams.set("start", startNanoseconds.toString());
        url.searchParams.set("end", endNanoseconds.toString());
        url.searchParams.set("direction", request.direction);
        url.searchParams.set("limit", String(request.limit));
        return parseLokiResponse(await this.fetchJson(url, signal));
      }),
    );
    const seen = new Set<string>();
    const failures: BackendFailure[] = [];
    const observedAt = new Date().toISOString();
    const logs = responses.flatMap((response, index) => {
      if (response.status === "fulfilled") return response.value;
      failures.push({
        source: "loki",
        message: `Log sample ${index + 1} failed: ${errorMessage(response.reason)}`,
        observedAt,
      });
      return [];
    }).filter((entry) => {
      if (seen.has(entry.id)) return false;
      seen.add(entry.id);
      return true;
    });
    return { logs, failures };
  }

  private async querySpans(
    since: Date,
    signal?: AbortSignal,
  ): Promise<readonly TelemetrySpan[]> {
    const url = new URL("/api/search", this.endpoints.tempoUrl);
    url.searchParams.set(
      "q",
      '{ resource.service.namespace = "flaggo" && name =~ "^flaggo\\\\..*" }',
    );
    url.searchParams.set("start", String(Math.floor(since.getTime() / 1000)));
    url.searchParams.set("end", String(Math.ceil(Date.now() / 1000)));
    url.searchParams.set("limit", "500");
    return parseTempoResponse(await this.fetchJson(url, signal));
  }

  private async queryMetrics(
    signal?: AbortSignal,
  ): Promise<readonly MetricView[]> {
    const url = new URL("/api/v1/query", this.endpoints.prometheusUrl);
    url.searchParams.set("query", '{__name__=~"flaggo_.*"}');
    return parsePrometheusResponse(await this.fetchJson(url, signal));
  }

  private async fetchJson(url: URL, signal?: AbortSignal): Promise<unknown> {
    const deadline = AbortSignal.timeout(2_000);
    const response = await this.fetchImpl(url, {
      headers: { Accept: "application/json" },
      signal: signal === undefined ? deadline : AbortSignal.any([signal, deadline]),
    });
    const body = await response.text();
    if (!response.ok) {
      throw new Error(
        `${url.origin} returned HTTP ${response.status}: ${compact(body)}`,
      );
    }
    try {
      return JSON.parse(body);
    } catch (error) {
      throw new Error(`${url.origin} returned invalid JSON.`, { cause: error });
    }
  }
}

export function parseLokiResponse(value: unknown): readonly TelemetryLog[] {
  const root = record(value);
  const data = record(root?.data);
  const streams = Array.isArray(data?.result) ? data.result : [];
  const logs: TelemetryLog[] = [];
  for (const candidate of streams) {
    const stream = record(candidate);
    const streamAttributes = normalizeAttributes(record(stream?.stream));
    const values = Array.isArray(stream?.values) ? stream.values : [];
    for (const raw of values) {
      if (!Array.isArray(raw) || raw.length < 2) continue;
      const nanoseconds = typeof raw[0] === "string" ? raw[0] : undefined;
      const line = typeof raw[1] === "string" ? raw[1] : "";
      if (nanoseconds === undefined) continue;
      const lineAttributes = parseLineAttributes(line);
      const metadata = normalizeAttributes(record(raw[2]));
      const attributes = {
        ...streamAttributes,
        ...lineAttributes,
        ...metadata,
      };
      const observedAt = nanosecondsToIso(nanoseconds);
      const eventName = stringValue(attributes["event.name"]);
      const service = stringValue(attributes["service.name"])
        ?? stringValue(attributes.service_name)
        ?? "unknown";
      const traceId = stringValue(
        attributes.trace_id ?? attributes.traceId,
      );
      const spanId = stringValue(attributes.span_id ?? attributes.spanId);
      const severity = stringValue(attributes.severity_text);
      logs.push({
        id: `log:${nanoseconds}:${service}:${eventName ?? line}`,
        observedAt,
        service,
        ...(eventName === undefined ? {} : { eventName }),
        ...(severity === undefined ? {} : { severity }),
        message: line,
        ...(traceId === undefined ? {} : { traceId }),
        ...(spanId === undefined ? {} : { spanId }),
        attributes,
      });
    }
  }
  return logs;
}

export function parseTempoResponse(value: unknown): readonly TelemetrySpan[] {
  const root = record(value);
  const traces = Array.isArray(root?.traces) ? root.traces : [];
  const spans: TelemetrySpan[] = [];
  for (const rawTrace of traces) {
    const trace = record(rawTrace);
    const traceId = stringValue(trace?.traceID ?? trace?.traceId);
    if (traceId === undefined) continue;
    const rootService = stringValue(
      trace?.rootServiceName ?? trace?.rootService,
    ) ?? "unknown";
    const spanSets = Array.isArray(trace?.spanSets) ? trace.spanSets : [];
    for (const rawSet of spanSets) {
      const spanSet = record(rawSet);
      const candidates = Array.isArray(spanSet?.spans) ? spanSet.spans : [];
      for (const rawSpan of candidates) {
        const span = record(rawSpan);
        const name = stringValue(span?.name);
        const start = stringValue(
          span?.startTimeUnixNanos ?? span?.startTimeUnixNano,
        );
        if (name === undefined || start === undefined) continue;
        const attributes = normalizeTempoAttributes(span?.attributes);
        const scopeName = stringValue(
          span?.scopeName ?? span?.instrumentationScopeName,
        );
        if (scopeName !== undefined) {
          attributes["otel.scope.name"] = scopeName;
        }
        const service = stringValue(attributes["service.name"])
          ?? stringValue(attributes.service_name)
          ?? rootService;
        const spanId = stringValue(span?.spanID ?? span?.spanId);
        const durationNanoseconds = numericValue(
          span?.durationNanos ?? span?.durationNano,
        );
        const durationMilliseconds = numericValue(span?.durationMs);
        spans.push({
          id: `span:${traceId}:${spanId ?? name}:${start}`,
          observedAt: nanosecondsToIso(start),
          service,
          name,
          ...(durationNanoseconds === undefined
            ? durationMilliseconds === undefined
              ? {}
              : { durationMs: durationMilliseconds }
            : { durationMs: durationNanoseconds / 1_000_000 }),
          traceId,
          ...(spanId === undefined ? {} : { spanId }),
          attributes,
        });
      }
    }
  }
  return spans;
}

export function parsePrometheusResponse(value: unknown): readonly MetricView[] {
  const root = record(value);
  const data = record(root?.data);
  const candidates = Array.isArray(data?.result) ? data.result : [];
  const metrics: MetricView[] = [];
  for (const candidate of candidates) {
    const result = record(candidate);
    const labels = record(result?.metric);
    const sample = Array.isArray(result?.value) ? result.value : undefined;
    const name = stringValue(labels?.__name__);
    const number = sample === undefined ? undefined : numericValue(sample[1]);
    if (name === undefined || number === undefined) continue;
    const normalizedLabels = Object.fromEntries(
      Object.entries(labels ?? {})
        .filter(([key, item]) => key !== "__name__" && typeof item === "string")
        .map(([key, item]) => [key, item as string]),
    );
    metrics.push({
      name,
      ...(normalizedLabels.service_name === undefined
        ? {}
        : { service: normalizedLabels.service_name }),
      value: number,
      labels: normalizedLabels,
    });
  }
  return metrics;
}

function normalizeTempoAttributes(
  value: unknown,
): Record<string, string | number | boolean> {
  if (Array.isArray(value)) {
    return normalizeAttributes(Object.fromEntries(value.flatMap((candidate) => {
      const attribute = record(candidate);
      const key = stringValue(attribute?.key);
      const item = primitiveValue(attribute?.value);
      return key === undefined || item === undefined ? [] : [[key, item]];
    })));
  }
  return normalizeAttributes(record(value));
}

function normalizeAttributes(
  value: Record<string, unknown> | undefined,
): Record<string, string | number | boolean> {
  const normalized: Record<string, string | number | boolean> = {};
  for (const [rawName, rawValue] of Object.entries(value ?? {})) {
    const item = primitiveValue(rawValue);
    if (item === undefined) continue;
    const name = normalizedAttributeNames.get(rawName) ?? rawName;
    normalized[name] = item;
  }
  return normalized;
}

function parseLineAttributes(
  line: string,
): Record<string, string | number | boolean> {
  try {
    const parsed = record(JSON.parse(line));
    return normalizeAttributes(parsed);
  } catch {
    return {};
  }
}

function primitiveValue(value: unknown): string | number | boolean | undefined {
  if (
    typeof value === "string"
    || typeof value === "number"
    || typeof value === "boolean"
  ) {
    return value;
  }
  const typed = record(value);
  return primitiveValue(typed?.stringValue)
    ?? primitiveValue(typed?.intValue)
    ?? primitiveValue(typed?.doubleValue)
    ?? primitiveValue(typed?.boolValue)
    ?? primitiveValue(typed?.value);
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function nanosecondsToIso(value: string): string {
  try {
    return new Date(Number(BigInt(value) / 1_000_000n)).toISOString();
  } catch {
    return new Date(0).toISOString();
  }
}

function numericValue(value: unknown): number | undefined {
  const number = typeof value === "number"
    ? value
    : typeof value === "string"
      ? Number(value)
      : undefined;
  return number !== undefined && Number.isFinite(number) ? number : undefined;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

function settledValue<T>(
  result: PromiseSettledResult<T>,
  fallback: T,
): T {
  return result.status === "fulfilled" ? result.value : fallback;
}

function compact(value: string): string {
  const normalized = value.replace(/\s+/gu, " ").trim();
  return normalized.length <= 240
    ? normalized
    : `${normalized.slice(0, 237)}...`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
