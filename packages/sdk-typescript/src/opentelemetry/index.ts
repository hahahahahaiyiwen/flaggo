import { ExportResultCode, type ExportResult } from "@opentelemetry/core";
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-http";
import {
  OTLPMetricExporter,
  type OTLPMetricExporterOptions,
} from "@opentelemetry/exporter-metrics-otlp-http";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import {
  CompressionAlgorithm,
  type OTLPExporterNodeConfigBase,
} from "@opentelemetry/otlp-exporter-base";
import {
  resourceFromAttributes,
  type Resource,
} from "@opentelemetry/resources";
import {
  BatchLogRecordProcessor,
  type BatchLogRecordProcessorOptions,
  type LogRecordExporter,
  type LogRecordProcessor,
  type ReadableLogRecord,
} from "@opentelemetry/sdk-logs";
import {
  PeriodicExportingMetricReader,
  type MetricData,
  type PeriodicExportingMetricReaderOptions,
  type PushMetricExporter,
  type ResourceMetrics,
  type ScopeMetrics,
} from "@opentelemetry/sdk-metrics";
import {
  BatchSpanProcessor,
  type BatchSpanProcessorOptions,
  type ReadableSpan,
  type SpanExporter,
  type SpanProcessor,
} from "@opentelemetry/sdk-trace";
import { parseFlaggoRuntimeConfiguration } from "../configuration/configuration.js";
import type {
  AuthorityScope,
  FlaggoRuntimeConfiguration,
} from "../configuration/types.js";

export { SDK_VERSION } from "../generated/package-version.generated.js";
export { CompressionAlgorithm };

type OtlpTransportOptions = Pick<
  OTLPExporterNodeConfigBase,
  "compression" | "concurrencyLimit" | "headers" | "timeoutMillis"
>;

export interface FlaggoOtlpOptions extends OtlpTransportOptions {
  readonly runtimeConfig: FlaggoRuntimeConfiguration;
}

export interface FlaggoResourceOptions {
  readonly baseResource?: Resource;
  readonly runtimeConfig: FlaggoRuntimeConfiguration;
}

export type FlaggoLogSelector = (record: ReadableLogRecord) => boolean;
export type FlaggoSpanSelector = (span: ReadableSpan) => boolean;

export interface FlaggoMetricSelectorContext {
  readonly instrumentationScope: ScopeMetrics["scope"];
  readonly resource: ResourceMetrics["resource"];
}

export type FlaggoMetricSelector = (
  metric: MetricData,
  context: FlaggoMetricSelectorContext,
) => boolean;

export interface FlaggoLogExporterOptions extends FlaggoOtlpOptions {
  readonly shouldExport?: FlaggoLogSelector;
}

export interface FlaggoTraceExporterOptions extends FlaggoOtlpOptions {
  readonly shouldExport?: FlaggoSpanSelector;
}

export interface FlaggoMetricExporterOptions extends FlaggoOtlpOptions {
  readonly aggregationPreference?:
    OTLPMetricExporterOptions["aggregationPreference"];
  readonly temporalityPreference?:
    OTLPMetricExporterOptions["temporalityPreference"];
  readonly shouldExport?: FlaggoMetricSelector;
}

export interface FlaggoLogRecordProcessorOptions
  extends FlaggoLogExporterOptions {
  readonly batch?: Omit<BatchLogRecordProcessorOptions, "exporter">;
}

export interface FlaggoSpanProcessorOptions
  extends FlaggoTraceExporterOptions {
  readonly batch?: Omit<BatchSpanProcessorOptions, "exporter">;
}

export interface FlaggoMetricReaderOptions
  extends FlaggoMetricExporterOptions {
  readonly periodic?: Omit<
    PeriodicExportingMetricReaderOptions,
    "exporter"
  >;
}

export function createFlaggoResource(
  options: FlaggoResourceOptions,
): Resource {
  const runtimeConfig = parseFlaggoRuntimeConfiguration(options.runtimeConfig);
  const authority = authorityAttributes(runtimeConfig.authority);
  if (options.baseResource !== undefined) {
    assertResourceAuthority(options.baseResource, authority, true);
  }
  const authorityResource = resourceFromAttributes(authority);
  return options.baseResource === undefined
    ? authorityResource
    : options.baseResource.merge(authorityResource);
}

export function createFlaggoLogExporter(
  options: FlaggoLogExporterOptions,
): LogRecordExporter {
  const runtimeConfig = parseFlaggoRuntimeConfiguration(options.runtimeConfig);
  const exporter: LogRecordExporter = new OTLPLogExporter(
    transportConfiguration(
      options,
      runtimeConfig.services.otlpIngestionUrl,
      "/v1/logs",
    ),
  );
  return new FlaggoLogRecordExporter(
    exporter,
    authorityAttributes(runtimeConfig.authority),
    options.shouldExport,
  );
}

export function createFlaggoTraceExporter(
  options: FlaggoTraceExporterOptions,
): SpanExporter {
  const runtimeConfig = parseFlaggoRuntimeConfiguration(options.runtimeConfig);
  const exporter: SpanExporter = new OTLPTraceExporter(
    transportConfiguration(
      options,
      runtimeConfig.services.otlpIngestionUrl,
      "/v1/traces",
    ),
  );
  return new FlaggoSpanExporter(
    exporter,
    authorityAttributes(runtimeConfig.authority),
    options.shouldExport,
  );
}

export function createFlaggoMetricExporter(
  options: FlaggoMetricExporterOptions,
): PushMetricExporter {
  const runtimeConfig = parseFlaggoRuntimeConfiguration(options.runtimeConfig);
  const exporter: PushMetricExporter = new OTLPMetricExporter({
    ...transportConfiguration(
      options,
      runtimeConfig.services.otlpIngestionUrl,
      "/v1/metrics",
    ),
    ...(options.aggregationPreference === undefined
      ? {}
      : { aggregationPreference: options.aggregationPreference }),
    ...(options.temporalityPreference === undefined
      ? {}
      : { temporalityPreference: options.temporalityPreference }),
  });
  return flaggoMetricExporter(
    exporter,
    authorityAttributes(runtimeConfig.authority),
    options.shouldExport,
  );
}

export function createFlaggoLogRecordProcessor(
  options: FlaggoLogRecordProcessorOptions,
): LogRecordProcessor {
  return new BatchLogRecordProcessor({
    ...options.batch,
    exporter: createFlaggoLogExporter(options),
  });
}

export function createFlaggoSpanProcessor(
  options: FlaggoSpanProcessorOptions,
): SpanProcessor {
  return new BatchSpanProcessor({
    ...options.batch,
    exporter: createFlaggoTraceExporter(options),
  });
}

export function createFlaggoMetricReader(
  options: FlaggoMetricReaderOptions,
): PeriodicExportingMetricReader {
  return new PeriodicExportingMetricReader({
    ...options.periodic,
    exporter: createFlaggoMetricExporter(options),
  });
}

function transportConfiguration(
  options: FlaggoOtlpOptions,
  baseUrl: string,
  signalPath: `/${string}`,
): OTLPExporterNodeConfigBase {
  return {
    url: signalEndpoint(baseUrl, signalPath),
    ...(options.compression === undefined
      ? {}
      : { compression: options.compression }),
    ...(options.concurrencyLimit === undefined
      ? {}
      : { concurrencyLimit: options.concurrencyLimit }),
    ...(options.headers === undefined ? {} : { headers: options.headers }),
    ...(options.timeoutMillis === undefined
      ? {}
      : { timeoutMillis: options.timeoutMillis }),
  };
}

function signalEndpoint(
  baseUrl: string | URL,
  signalPath: `/${string}`,
): string {
  let endpoint: URL;
  try {
    endpoint = new URL(baseUrl);
  } catch (error) {
    throw new TypeError("Flaggo OTLP baseUrl must be an absolute URL.", {
      cause: error,
    });
  }
  if (endpoint.protocol !== "http:" && endpoint.protocol !== "https:") {
    throw new TypeError("Flaggo OTLP baseUrl must use HTTP or HTTPS.");
  }
  if (endpoint.username !== "" || endpoint.password !== "") {
    throw new TypeError("Flaggo OTLP baseUrl must not contain credentials.");
  }
  if (endpoint.search !== "" || endpoint.hash !== "") {
    throw new TypeError(
      "Flaggo OTLP baseUrl must not contain a query string or fragment.",
    );
  }

  endpoint.pathname = `${endpoint.pathname.replace(/\/+$/u, "")}${signalPath}`;
  return endpoint.href;
}

class FlaggoLogRecordExporter implements LogRecordExporter {
  constructor(
    private readonly exporter: LogRecordExporter,
    private readonly authority: Readonly<Record<string, string>>,
    private readonly shouldExport: FlaggoLogSelector | undefined,
  ) {}

  export(
    records: ReadableLogRecord[],
    resultCallback: (result: ExportResult) => void,
  ): void {
    let selected: ReadableLogRecord[];
    try {
      for (const record of records) {
        assertResourceAuthority(record.resource, this.authority, false);
      }
      selected = this.shouldExport === undefined
        ? records
        : records.filter(this.shouldExport);
    } catch (error) {
      resultCallback(failedExport(error));
      return;
    }
    if (selected.length === 0) {
      resultCallback({ code: ExportResultCode.SUCCESS });
      return;
    }
    this.exporter.export(selected, resultCallback);
  }

  forceFlush(): Promise<void> {
    return this.exporter.forceFlush();
  }

  shutdown(): Promise<void> {
    return this.exporter.shutdown();
  }
}

class FlaggoSpanExporter implements SpanExporter {
  constructor(
    private readonly exporter: SpanExporter,
    private readonly authority: Readonly<Record<string, string>>,
    private readonly shouldExport: FlaggoSpanSelector | undefined,
  ) {}

  export(
    spans: ReadableSpan[],
    resultCallback: (result: ExportResult) => void,
  ): void {
    let selected: ReadableSpan[];
    try {
      for (const span of spans) {
        assertResourceAuthority(span.resource, this.authority, false);
      }
      selected = this.shouldExport === undefined
        ? spans
        : spans.filter(this.shouldExport);
    } catch (error) {
      resultCallback(failedExport(error));
      return;
    }
    if (selected.length === 0) {
      resultCallback({ code: ExportResultCode.SUCCESS });
      return;
    }
    this.exporter.export(selected, resultCallback);
  }

  forceFlush(): Promise<void> {
    return this.exporter.forceFlush?.() ?? Promise.resolve();
  }

  shutdown(): Promise<void> {
    return this.exporter.shutdown();
  }
}

function flaggoMetricExporter(
  exporter: PushMetricExporter,
  authority: Readonly<Record<string, string>>,
  shouldExport: FlaggoMetricSelector | undefined,
): PushMetricExporter {
  return {
    export(metrics, resultCallback) {
      let scopeMetrics: ScopeMetrics[];
      try {
        assertResourceAuthority(metrics.resource, authority, false);
        scopeMetrics = metrics.scopeMetrics.flatMap((scope) => {
          const selected = shouldExport === undefined
            ? scope.metrics
            : scope.metrics.filter((metric) =>
              shouldExport(metric, {
                instrumentationScope: scope.scope,
                resource: metrics.resource,
              })
            );
          return selected.length === 0
            ? []
            : [{ scope: scope.scope, metrics: selected }];
        });
      } catch (error) {
        resultCallback(failedExport(error));
        return;
      }
      if (scopeMetrics.length === 0) {
        resultCallback({ code: ExportResultCode.SUCCESS });
        return;
      }
      exporter.export(
        { resource: metrics.resource, scopeMetrics },
        resultCallback,
      );
    },
    forceFlush() {
      return exporter.forceFlush();
    },
    shutdown() {
      return exporter.shutdown();
    },
    ...(exporter.selectAggregation === undefined
      ? {}
      : {
        selectAggregation:
          exporter.selectAggregation.bind(exporter),
      }),
    ...(exporter.selectAggregationTemporality === undefined
      ? {}
      : {
        selectAggregationTemporality:
          exporter.selectAggregationTemporality.bind(exporter),
      }),
  };
}

function authorityAttributes(
  authority: AuthorityScope,
): Readonly<Record<string, string>> {
  return Object.freeze({
    "flaggo.tenant": authority.tenant,
    "flaggo.application": authority.application,
    "flaggo.environment": authority.environment,
  });
}

function assertResourceAuthority(
  resource: Resource,
  authority: Readonly<Record<string, string>>,
  allowMissing: boolean,
): void {
  for (const [name, expected] of Object.entries(authority)) {
    const actual = resource.attributes[name];
    if (allowMissing && actual === undefined) continue;
    if (actual !== expected) {
      throw new TypeError(
        `OpenTelemetry Resource attribute '${name}' must equal `
        + `'${expected}' for this Flaggo runtime configuration.`,
      );
    }
  }
}

function failedExport(error: unknown): ExportResult {
  return {
    code: ExportResultCode.FAILED,
    error: error instanceof Error ? error : new Error(String(error)),
  };
}
