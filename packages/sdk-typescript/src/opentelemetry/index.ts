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

export { SDK_VERSION } from "../generated/package-version.generated.js";
export { CompressionAlgorithm };

type OtlpTransportOptions = Pick<
  OTLPExporterNodeConfigBase,
  "compression" | "concurrencyLimit" | "headers" | "timeoutMillis"
>;

export interface FlaggoOtlpOptions extends OtlpTransportOptions {
  readonly baseUrl: string | URL;
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

export function createFlaggoLogExporter(
  options: FlaggoLogExporterOptions,
): LogRecordExporter {
  const exporter: LogRecordExporter = new OTLPLogExporter(
    transportConfiguration(options, "/v1/logs"),
  );
  return options.shouldExport === undefined
    ? exporter
    : new SelectingLogRecordExporter(exporter, options.shouldExport);
}

export function createFlaggoTraceExporter(
  options: FlaggoTraceExporterOptions,
): SpanExporter {
  const exporter: SpanExporter = new OTLPTraceExporter(
    transportConfiguration(options, "/v1/traces"),
  );
  return options.shouldExport === undefined
    ? exporter
    : new SelectingSpanExporter(exporter, options.shouldExport);
}

export function createFlaggoMetricExporter(
  options: FlaggoMetricExporterOptions,
): PushMetricExporter {
  const exporter: PushMetricExporter = new OTLPMetricExporter({
    ...transportConfiguration(options, "/v1/metrics"),
    ...(options.aggregationPreference === undefined
      ? {}
      : { aggregationPreference: options.aggregationPreference }),
    ...(options.temporalityPreference === undefined
      ? {}
      : { temporalityPreference: options.temporalityPreference }),
  });
  return options.shouldExport === undefined
    ? exporter
    : selectingMetricExporter(exporter, options.shouldExport);
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
  signalPath: `/${string}`,
): OTLPExporterNodeConfigBase {
  return {
    url: signalEndpoint(options.baseUrl, signalPath),
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

class SelectingLogRecordExporter implements LogRecordExporter {
  constructor(
    private readonly exporter: LogRecordExporter,
    private readonly shouldExport: FlaggoLogSelector,
  ) {}

  export(
    records: ReadableLogRecord[],
    resultCallback: (result: ExportResult) => void,
  ): void {
    let selected: ReadableLogRecord[];
    try {
      selected = records.filter(this.shouldExport);
    } catch (error) {
      resultCallback(failedSelection(error));
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

class SelectingSpanExporter implements SpanExporter {
  constructor(
    private readonly exporter: SpanExporter,
    private readonly shouldExport: FlaggoSpanSelector,
  ) {}

  export(
    spans: ReadableSpan[],
    resultCallback: (result: ExportResult) => void,
  ): void {
    let selected: ReadableSpan[];
    try {
      selected = spans.filter(this.shouldExport);
    } catch (error) {
      resultCallback(failedSelection(error));
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

function selectingMetricExporter(
  exporter: PushMetricExporter,
  shouldExport: FlaggoMetricSelector,
): PushMetricExporter {
  return {
    export(metrics, resultCallback) {
      let scopeMetrics: ScopeMetrics[];
      try {
        scopeMetrics = metrics.scopeMetrics.flatMap((scope) => {
          const selected = scope.metrics.filter((metric) =>
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
        resultCallback(failedSelection(error));
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

function failedSelection(error: unknown): ExportResult {
  return {
    code: ExportResultCode.FAILED,
    error: error instanceof Error ? error : new Error(String(error)),
  };
}
