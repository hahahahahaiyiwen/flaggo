import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import type { Logger, LogRecord } from "@opentelemetry/api-logs";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  InMemoryLogRecordExporter,
  LoggerProvider,
  SimpleLogRecordProcessor,
} from "@opentelemetry/sdk-logs";

export type ApplicationLogger = Pick<Logger, "emit">;

export class LocalOtelLogs implements ApplicationLogger {
  private readonly exporter = new InMemoryLogRecordExporter();
  private readonly provider = new LoggerProvider({
    resource: resourceFromAttributes({ "service.name": "adaptive-worker" }),
    processors: [new SimpleLogRecordProcessor({ exporter: this.exporter })],
  });
  private readonly logger = this.provider.getLogger("adaptive-worker", "1");

  constructor(private readonly filePath?: string) {}

  get events() {
    return this.exporter.getFinishedLogRecords().map((record) => ({
      eventName: record.eventName,
      body: record.body,
      attributes: record.attributes,
      timeUnixNano: (BigInt(record.hrTime[0]) * 1_000_000_000n + BigInt(record.hrTime[1])).toString(),
    }));
  }

  emit(record: LogRecord): void {
    this.logger.emit(record);
  }

  async flush(): Promise<void> {
    await this.provider.forceFlush();
    if (this.filePath !== undefined) {
      mkdirSync(dirname(this.filePath), { recursive: true });
      writeFileSync(
        this.filePath,
        this.events.map((record) => `${JSON.stringify(record)}\n`).join(""),
        "utf8",
      );
    }
  }

  async shutdown(): Promise<void> {
    await this.flush();
    await this.provider.shutdown();
  }
}
