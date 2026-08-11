import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

import type { TelemetryEvent, TelemetrySink } from "@flaggo/sdk";

export class LocalTelemetrySink implements TelemetrySink {
  readonly events: TelemetryEvent[] = [];
  private readonly filePath: string | undefined;

  constructor(filePath?: string) {
    this.filePath = filePath;
    if (filePath !== undefined) {
      mkdirSync(dirname(filePath), { recursive: true });
      writeFileSync(filePath, "", "utf8");
    }
  }

  emit(event: TelemetryEvent): void {
    const captured = structuredClone(event);
    this.events.push(captured);
    if (this.filePath !== undefined) {
      appendFileSync(
        this.filePath,
        `${JSON.stringify(captured)}\n`,
        "utf8",
      );
    }
  }
}
