import type { LogRecord } from "@opentelemetry/api-logs";
import { describe, expect, it, vi } from "vitest";

import {
  createDecisionClient,
  createFlaggoTelemetry,
  InvalidFlaggoInputError,
  type DecisionBindings,
  type DecisionSpec,
  type FetchLike,
  type FlaggoTelemetryLogger,
} from "../src/runtime/index.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";

type Decisions = {
  readonly parallelism: DecisionSpec<{
    readonly queuePressure: number;
  }, number>;
};

const bindings: DecisionBindings<Decisions> = {
  parallelism: { contractDigest },
};

function decision(result = 4): object {
  return {
    contractDigest,
    executableDigest,
    result,
    evaluation: { source: "rule", rule: "queue-pressure" },
  };
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: {
      "Content-Type": "application/json",
      "X-Flaggo-Correlation-Id": "server-correlation",
    },
  });
}

function captureLogger(): {
  readonly logger: FlaggoTelemetryLogger;
  readonly records: LogRecord[];
} {
  const records: LogRecord[] = [];
  return {
    records,
    logger: {
      emit(record) {
        records.push(record);
      },
    },
  };
}

describe("runtime telemetry", () => {
  it("emits decision-received telemetry after a successful decide response", async () => {
    const telemetry = captureLogger();
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(decision()));
    const client = createDecisionClient<Decisions>({
      baseUrl: "https://decisions.test/",
      bindings,
      fetch,
      random: () => 0.25,
      telemetry: { logger: telemetry.logger },
    });

    await client.decide("parallelism", {
      attributes: { queuePressure: 0.75 },
    });

    const record = expectSingle(telemetry.records);
    expect(record.eventName).toBe("flaggo.decision.received");
    expect(record.attributes).toMatchObject({
      "flaggo.signal": "decision.received",
      "flaggo.contract.name": "parallelism",
      "flaggo.contract.digest": contractDigest,
      "flaggo.executable.digest": executableDigest,
      "flaggo.result.json": "4",
      "flaggo.evaluation.source": "rule",
      "flaggo.evaluation.rule": "queue-pressure",
      "flaggo.request.correlation_id": "server-correlation",
    });
    expect(record.attributes?.["flaggo.decision.id"]).toEqual(expect.any(String));
    expect(record.attributes?.["flaggo.result.hash"]).toMatch(/^sha256:[0-9a-f]{64}$/u);
  });

  it("keeps multiple decisions individually identifiable", async () => {
    const telemetry = captureLogger();
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(jsonResponse(decision(4)))
      .mockResolvedValueOnce(jsonResponse(decision(8)));
    const client = createDecisionClient<Decisions>({
      baseUrl: "https://decisions.test/",
      bindings,
      fetch,
      telemetry: { logger: telemetry.logger },
    });

    await client.decide("parallelism");
    await client.decide("parallelism");

    expect(telemetry.records).toHaveLength(2);
    expect(telemetry.records[0]?.attributes?.["flaggo.decision.id"])
      .not.toBe(telemetry.records[1]?.attributes?.["flaggo.decision.id"]);
    expect(telemetry.records.map((record) => record.attributes?.["flaggo.result.json"]))
      .toEqual(["4", "8"]);
  });

  it("does not emit decision telemetry for failed decisions", async () => {
    const telemetry = captureLogger();
    const fetch = vi.fn<FetchLike>(async () => new Response(
      JSON.stringify({
        type: "https://flaggo.dev/problems/invalid-runtime-input",
        title: "Invalid runtime input",
        status: 400,
      }),
      {
        status: 400,
        headers: { "Content-Type": "application/problem+json" },
      },
    ));
    const client = createDecisionClient<Decisions>({
      baseUrl: "https://decisions.test/",
      bindings,
      fetch,
      telemetry: { logger: telemetry.logger },
    });

    await expect(client.decide("parallelism")).rejects.toThrow();

    expect(telemetry.records).toEqual([]);
  });

  it("emits outcome telemetry through helper APIs", () => {
    const telemetry = captureLogger();
    const clientTelemetry = createFlaggoTelemetry({ logger: telemetry.logger });

    clientTelemetry.recordOutcome({
      binding: "worker.latency_ms",
      value: 125,
      decisionId: "decision-1",
      contractName: "parallelism",
      contractDigest,
      correlation: {
        workerId: "worker-1",
        queuePressure: 0.75,
      },
    });

    const record = expectSingle(telemetry.records);
    expect(record.eventName).toBe("flaggo.outcome.observed");
    expect(record.attributes).toEqual({
      "flaggo.signal": "outcome.observed",
      "flaggo.evidence.binding": "worker.latency_ms",
      "flaggo.evidence.value.json": "125",
      "flaggo.decision.id": "decision-1",
      "flaggo.contract.name": "parallelism",
      "flaggo.contract.digest": contractDigest,
      "flaggo.correlation.workerId": "worker-1",
      "flaggo.correlation.queuePressure": 0.75,
    });
  });

  it("rejects invalid outcome telemetry before emitting", () => {
    const telemetry = captureLogger();
    const clientTelemetry = createFlaggoTelemetry({ logger: telemetry.logger });

    expect(() => clientTelemetry.recordOutcome({
      binding: "invalid binding",
      value: true,
    })).toThrow(InvalidFlaggoInputError);
    expect(telemetry.records).toEqual([]);
  });
});

function expectSingle<T>(values: readonly T[]): T {
  expect(values).toHaveLength(1);
  return values[0]!;
}
