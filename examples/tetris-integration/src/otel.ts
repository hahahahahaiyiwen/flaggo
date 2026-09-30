import { context } from "@opentelemetry/api";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { resourceFromAttributes } from "@opentelemetry/resources";
import { LoggerProvider } from "@opentelemetry/sdk-logs";
import { MeterProvider } from "@opentelemetry/sdk-metrics";
import { TracerProvider } from "@opentelemetry/sdk-trace";
import {
  createFlaggoLogRecordProcessor,
  createFlaggoMetricReader,
  createFlaggoSpanProcessor,
} from "@flaggo/sdk/opentelemetry";
import { SDK_VERSION } from "@flaggo/sdk/runtime";

import type { TetrisOpenTelemetry } from "./flaggo-provider.js";

export interface TetrisTelemetryOptions {
  readonly environment?: string;
  readonly flaggoOtlpBaseUrl: string | URL;
}

export class TetrisTelemetryProviders {
  readonly instrumentation: TetrisOpenTelemetry;

  private readonly contextManager: AsyncLocalStorageContextManager | undefined;
  private readonly loggerProvider: LoggerProvider;
  private readonly meterProvider: MeterProvider;
  private readonly tracerProvider: TracerProvider;

  constructor(options: TetrisTelemetryOptions) {
    const resource = resourceFromAttributes({
      "service.name": "tetris",
      "deployment.environment.name": options.environment ?? "local",
    });
    this.loggerProvider = new LoggerProvider({
      resource,
      processors: [
        createFlaggoLogRecordProcessor({
          baseUrl: options.flaggoOtlpBaseUrl,
        }),
      ],
    });
    this.meterProvider = new MeterProvider({
      resource,
      readers: [
        createFlaggoMetricReader({
          baseUrl: options.flaggoOtlpBaseUrl,
          periodic: { exportIntervalMillis: 5_000 },
        }),
      ],
    });
    this.tracerProvider = new TracerProvider({
      resource,
      spanProcessors: [
        createFlaggoSpanProcessor({
          baseUrl: options.flaggoOtlpBaseUrl,
        }),
      ],
    });

    const contextManager = new AsyncLocalStorageContextManager().enable();
    if (context.setGlobalContextManager(contextManager)) {
      this.contextManager = contextManager;
    } else {
      contextManager.disable();
      this.contextManager = undefined;
    }

    this.instrumentation = {
      flaggoLogger: this.loggerProvider.getLogger("@flaggo/sdk", SDK_VERSION),
      logger: this.loggerProvider.getLogger("tetris.app"),
      meter: this.meterProvider.getMeter("tetris.app"),
      tracer: this.tracerProvider.getTracer("tetris.app"),
    };
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
