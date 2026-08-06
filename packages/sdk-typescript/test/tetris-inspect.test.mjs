import { randomUUID } from "node:crypto";
import { createHash } from "node:crypto";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  inspectIntegration,
} from "../../../examples/tetris-integration/inspect.mjs";

describe("Tetris segmented audit inspection", () => {
  it("enumerates segments in durable manifest order", async () => {
    const root = resolve(
      import.meta.dirname,
      "../../../.flaggo/test-artifacts",
      `inspect-${randomUUID()}`,
    );
    const auditPath = join(root, "audit.jsonl");
    const telemetryPath = join(root, "telemetry.jsonl");
    const segments = join(`${auditPath}.d`, "segments");
    await mkdir(segments, { recursive: true });
    try {
      const firstName =
        `segment-${"1".padStart(20, "0")}-${"a".repeat(32)}.jsonl`;
      const secondName =
        `segment-${"2".padStart(20, "0")}-${"b".repeat(32)}.jsonl`;
      const firstContent =
        `${segmentHeader(1)}\n${exposure("exposure-1")}\n`;
      const secondContent =
        `${segmentHeader(2)}\n${exposure("exposure-2")}\n`;
      await writeFile(join(segments, firstName), firstContent);
      await writeFile(join(segments, secondName), secondContent);
      await writeFile(
        join(`${auditPath}.d`, "manifest.json"),
        `${JSON.stringify({
          version: 1,
          segments: [
            segmentCatalog(1, firstName, firstContent),
            segmentCatalog(2, secondName, secondContent),
          ],
        })}\n`,
      );
      await mkdir(dirname(telemetryPath), { recursive: true });
      await writeFile(telemetryPath, "");

      const inspection = await inspectIntegration(auditPath, telemetryPath);

      expect(inspection.exposures.map(({ exposureId }) => exposureId)).toEqual([
        "exposure-1",
        "exposure-2",
      ]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }

    function segmentCatalog(sequence, fileName, content) {
      return {
        sequence,
        fileName,
        length: Buffer.byteLength(content),
        contentHash:
          `sha256:${createHash("sha256").update(content).digest("hex")}`,
      };
    }
  });
});

function segmentHeader(sequence) {
  return JSON.stringify({
    kind: "segment",
    record: {
      version: 1,
      sequence,
      segmentId: `${sequence}`.padStart(32, "0"),
    },
  });
}

function exposure(exposureId) {
  return JSON.stringify({
    kind: "exposure",
    record: {
      exposureId,
      decisionId: `decision-${exposureId}`,
      appId: "tetris-demo",
      environment: "dev",
      confirmedAt: "2026-08-06T00:00:02Z",
    },
  });
}
