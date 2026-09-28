import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { Logger, LogRecord } from "@opentelemetry/api-logs";
import {
  createFlaggoLocalOtelLogger,
  type FlaggoLocalOtelLogger,
} from "@flaggo/sdk/runtime";

export type ApplicationLogger = Pick<Logger, "emit">;

export class LocalOtelLogs implements ApplicationLogger {
  private readonly logger: FlaggoLocalOtelLogger;

  constructor(
    private readonly filePath?: string,
    collectorLogsUrl = process.env.FLAGGO_OTEL_COLLECTOR_LOGS_URL,
  ) {
    this.logger = createFlaggoLocalOtelLogger({
      serviceName: "adaptive-worker",
      ...(collectorLogsUrl === undefined ? {} : { collectorLogsUrl }),
    });
  }

  get events() {
    return this.logger.events;
  }

  get otlpJson() {
    return this.logger.toOtlpJson();
  }

  emit(record: LogRecord): void {
    this.logger.emit(record);
  }

  async flush(): Promise<void> {
    await this.logger.flush();
    if (this.filePath !== undefined) {
      mkdirSync(dirname(this.filePath), { recursive: true });
      writeFileSync(
        this.filePath,
        `${JSON.stringify(this.otlpJson)}\n`,
        "utf8",
      );
    }
  }

  async shutdown(): Promise<void> {
    await this.flush();
    await this.logger.shutdown();
  }
}
