import {
  context,
  type Meter,
  type Tracer,
} from "@opentelemetry/api";
import type { Logger } from "@opentelemetry/api-logs";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  InMemoryLogRecordExporter,
  LoggerProvider,
  SimpleLogRecordProcessor,
  type ReadableLogRecord,
} from "@opentelemetry/sdk-logs";
import {
  AggregationTemporality,
  InMemoryMetricExporter,
  MeterProvider,
  PeriodicExportingMetricReader,
  type ResourceMetrics,
} from "@opentelemetry/sdk-metrics";
import {
  InMemorySpanExporter,
  SimpleSpanProcessor,
  TracerProvider,
  type ReadableSpan,
} from "@opentelemetry/sdk-trace";
import {
  createFlaggoResource,
  createFlaggoLogRecordProcessor,
  createFlaggoMetricReader,
  createFlaggoSpanProcessor,
} from "@flaggo/sdk/opentelemetry";
import type {
  FlaggoRuntimeConfiguration,
} from "@flaggo/sdk/configuration";
import { SDK_VERSION } from "@flaggo/sdk/runtime";

export interface ApplicationTelemetry {
  readonly flaggoLogger: Logger;
  readonly logger: Logger;
  readonly meter: Meter;
  readonly tracer: Tracer;
}

export interface AdaptiveWorkerTelemetryOptions {
  readonly capture?: boolean;
  readonly runtimeConfig: FlaggoRuntimeConfiguration;
}

export class AdaptiveWorkerTelemetry implements ApplicationTelemetry {
  readonly flaggoLogger: Logger;
  readonly logger: Logger;
  readonly meter: Meter;
  readonly tracer: Tracer;

  private readonly contextManager: AsyncLocalStorageContextManager | undefined;
  private readonly logExporter: InMemoryLogRecordExporter | undefined;
  private readonly loggerProvider: LoggerProvider;
  private readonly meterProvider: MeterProvider;
  private readonly metricExporter: InMemoryMetricExporter | undefined;
  private readonly spanExporter: InMemorySpanExporter | undefined;
  private readonly tracerProvider: TracerProvider;

  constructor(options: AdaptiveWorkerTelemetryOptions) {
    const resource = createFlaggoResource({
      baseResource: resourceFromAttributes({
        "service.name": "adaptive-worker",
        "deployment.environment.name":
          options.runtimeConfig.authority.environment,
      }),
      runtimeConfig: options.runtimeConfig,
    });
    this.logExporter = options.capture === true
      ? new InMemoryLogRecordExporter()
      : undefined;
    this.metricExporter = options.capture === true
      ? new InMemoryMetricExporter(AggregationTemporality.CUMULATIVE)
      : undefined;
    this.spanExporter = options.capture === true
      ? new InMemorySpanExporter()
      : undefined;

    this.loggerProvider = new LoggerProvider({
      resource,
      processors: [
        createFlaggoLogRecordProcessor({
          runtimeConfig: options.runtimeConfig,
        }),
        ...(this.logExporter === undefined
          ? []
          : [
            new SimpleLogRecordProcessor({
              exporter: this.logExporter,
            }),
          ]),
      ],
    });
    this.meterProvider = new MeterProvider({
      resource,
      readers: [
        createFlaggoMetricReader({
          runtimeConfig: options.runtimeConfig,
          periodic: { exportIntervalMillis: 5_000 },
        }),
        ...(this.metricExporter === undefined
          ? []
          : [
            new PeriodicExportingMetricReader({
              exporter: this.metricExporter,
              exportIntervalMillis: 60_000,
            }),
          ]),
      ],
    });
    this.tracerProvider = new TracerProvider({
      resource,
      spanProcessors: [
        createFlaggoSpanProcessor({
          runtimeConfig: options.runtimeConfig,
        }),
        ...(this.spanExporter === undefined
          ? []
          : [
            new SimpleSpanProcessor({
              exporter: this.spanExporter,
            }),
          ]),
      ],
    });

    const contextManager = new AsyncLocalStorageContextManager().enable();
    if (context.setGlobalContextManager(contextManager)) {
      this.contextManager = contextManager;
    } else {
      contextManager.disable();
      this.contextManager = undefined;
    }

    this.flaggoLogger = this.loggerProvider.getLogger(
      "@flaggo/sdk",
      SDK_VERSION,
    );
    this.logger = this.loggerProvider.getLogger("adaptive-worker.app");
    this.meter = this.meterProvider.getMeter("adaptive-worker.app");
    this.tracer = this.tracerProvider.getTracer("adaptive-worker.app");
  }

  get events(): readonly ReadableLogRecord[] {
    return this.logExporter?.getFinishedLogRecords() ?? [];
  }

  get metrics(): readonly ResourceMetrics[] {
    return this.metricExporter?.getMetrics() ?? [];
  }

  get spans(): readonly ReadableSpan[] {
    return this.spanExporter?.getFinishedSpans() ?? [];
  }

  async flush(): Promise<void> {
    await Promise.all([
      this.loggerProvider.forceFlush(),
      this.meterProvider.forceFlush(),
      this.tracerProvider.forceFlush(),
    ]);
  }

  async shutdown(): Promise<void> {
    try {
      await Promise.all([
        this.loggerProvider.shutdown(),
        this.meterProvider.shutdown(),
        this.tracerProvider.shutdown(),
      ]);
    } finally {
      this.contextManager?.disable();
    }
  }
}
