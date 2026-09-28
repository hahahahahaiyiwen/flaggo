import type { Logger } from "@opentelemetry/api-logs";
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-proto";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  InMemoryLogRecordExporter,
  LoggerProvider,
  type ReadableLogRecord,
  SimpleLogRecordProcessor,
} from "@opentelemetry/sdk-logs";

import { SDK_VERSION } from "../generated/package-version.generated.js";
import {
  hasOnlyKeys,
  inputError,
  record,
} from "../internal/guards.js";

export type FlaggoOtlpLogger = Pick<Logger, "emit"> & {
  readonly events: readonly FlaggoOtlpEvent[];
  flush(): Promise<void>;
  shutdown(): Promise<void>;
  toOtlpJson(): FlaggoOtlpJsonLogs;
};

export interface FlaggoOtlpLoggerConfiguration {
  readonly serviceName: string;
  readonly serviceVersion?: string;
  readonly collectorLogsUrl?: string;
}

export interface FlaggoOtlpEvent {
  readonly eventName?: string;
  readonly body?: unknown;
  readonly attributes?: ReadableLogRecord["attributes"];
  readonly timeUnixNano: string;
}

export interface FlaggoOtlpJsonLogs {
  readonly resourceLogs: readonly [{
    readonly resource: {
      readonly attributes: readonly OtlpAttribute[];
    };
    readonly scopeLogs: readonly [{
      readonly scope: {
        readonly name: string;
        readonly version: string;
      };
      readonly logRecords: readonly OtlpJsonLogRecord[];
    }];
  }];
}

type OtlpAttribute = {
  readonly key: string;
  readonly value: OtlpAnyValue;
};

type OtlpAnyValue =
  | { readonly stringValue: string }
  | { readonly boolValue: boolean }
  | { readonly intValue: string }
  | { readonly doubleValue: number };

type OtlpJsonLogRecord = {
  readonly timeUnixNano: string;
  readonly eventName?: string;
  readonly body?: OtlpAnyValue;
  readonly attributes: readonly OtlpAttribute[];
};

export function createFlaggoOtlpLogger(
  configuration: FlaggoOtlpLoggerConfiguration,
): FlaggoOtlpLogger {
  validateConfiguration(configuration);
  const inMemoryExporter = new InMemoryLogRecordExporter();
  const processors = [
    new SimpleLogRecordProcessor({ exporter: inMemoryExporter }),
  ];
  if (configuration.collectorLogsUrl !== undefined) {
    processors.push(new SimpleLogRecordProcessor({
      exporter: new OTLPLogExporter({ url: configuration.collectorLogsUrl }),
    }));
  }

  const provider = new LoggerProvider({
    resource: resourceFromAttributes({
      "service.name": configuration.serviceName,
      ...(configuration.serviceVersion === undefined
        ? {}
        : { "service.version": configuration.serviceVersion }),
    }),
    processors,
  });
  const logger = provider.getLogger("@flaggo/sdk", SDK_VERSION);

  return {
    get events() {
      return recordsToEvents(inMemoryExporter.getFinishedLogRecords());
    },
    emit(record) {
      logger.emit(record);
    },
    async flush() {
      await provider.forceFlush();
    },
    async shutdown() {
      await provider.forceFlush();
      await provider.shutdown();
    },
    toOtlpJson() {
      return eventsToOtlpJson(
        configuration,
        recordsToEvents(inMemoryExporter.getFinishedLogRecords()),
      );
    },
  };
}

function validateConfiguration(configuration: FlaggoOtlpLoggerConfiguration): void {
  const raw = record(configuration);
  if (
    raw === undefined
    || !hasOnlyKeys(
      raw,
      new Set(["serviceName", "serviceVersion", "collectorLogsUrl"]),
    )
  ) {
    inputError("/", "Local OTel configuration contains unknown members.");
  }
  if (configuration.serviceName.trim().length === 0) {
    inputError("/serviceName", "Service name is required.");
  }
  if (
    configuration.collectorLogsUrl !== undefined
    && !isHttpUrl(configuration.collectorLogsUrl)
  ) {
    inputError(
      "/collectorLogsUrl",
      "Collector logs URL must be an absolute HTTP(S) URL.",
    );
  }
}

function recordsToEvents(records: readonly ReadableLogRecord[]): readonly FlaggoOtlpEvent[] {
  return records.map((record) => ({
    ...(record.eventName === undefined ? {} : { eventName: record.eventName }),
    ...(record.body === undefined ? {} : { body: record.body }),
    ...(record.attributes === undefined ? {} : { attributes: record.attributes }),
    timeUnixNano: (
      BigInt(record.hrTime[0]) * 1_000_000_000n
      + BigInt(record.hrTime[1])
    ).toString(),
  }));
}

function eventsToOtlpJson(
  configuration: FlaggoOtlpLoggerConfiguration,
  events: readonly FlaggoOtlpEvent[],
): FlaggoOtlpJsonLogs {
  return {
    resourceLogs: [{
      resource: {
        attributes: [
          otlpAttribute("service.name", configuration.serviceName),
          ...(configuration.serviceVersion === undefined
            ? []
            : [otlpAttribute("service.version", configuration.serviceVersion)]),
        ],
      },
      scopeLogs: [{
        scope: {
          name: "@flaggo/sdk",
          version: SDK_VERSION,
        },
        logRecords: events.map((event) => ({
          timeUnixNano: event.timeUnixNano,
          ...(event.eventName === undefined ? {} : { eventName: event.eventName }),
          ...(event.body === undefined ? {} : { body: otlpValue(event.body) }),
          attributes: Object.entries(event.attributes ?? {}).map(([key, value]) =>
            otlpAttribute(key, value)),
        })),
      }],
    }],
  };
}

function otlpAttribute(key: string, value: unknown): OtlpAttribute {
  return {
    key,
    value: otlpValue(value),
  };
}

function otlpValue(value: unknown): OtlpAnyValue {
  if (typeof value === "boolean") {
    return { boolValue: value };
  }
  if (typeof value === "number") {
    return Number.isInteger(value)
      ? { intValue: String(value) }
      : { doubleValue: value };
  }
  if (typeof value === "bigint") {
    return { intValue: value.toString() };
  }
  if (typeof value === "string") {
    return { stringValue: value };
  }
  return { stringValue: JSON.stringify(value) };
}

function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}
