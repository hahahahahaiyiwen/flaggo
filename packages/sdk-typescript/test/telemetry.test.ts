import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { LogRecord } from "@opentelemetry/api-logs";
import addFormatsModule from "ajv-formats";
import { Ajv2020 } from "ajv/dist/2020.js";
import { describe, expect, it, vi } from "vitest";

import {
  createDecisionClient,
  recordDecisionReceived,
  type DecisionBindings,
  type DecisionSpec,
  type FetchLike,
  type FlaggoTelemetryLogger,
  type RuntimeDecision,
} from "../src/runtime/index.js";
import { runtimeConfiguration } from "./runtime-configuration.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const repositoryRoot = resolve(import.meta.dirname, "../../..");
const telemetryEventValidator = addFormatsModule.default(new Ajv2020({
  allErrors: true,
  strict: true,
})).compile(JSON.parse(readFileSync(
  resolve(
    repositoryRoot,
    "contracts/schemas/telemetry-events-v1.schema.json",
  ),
  "utf8",
)) as object);

type Decisions = {
  readonly parallelism: DecisionSpec<{
    readonly queuePressure: number;
  }, number>;
};

const bindings: DecisionBindings<Decisions> = {
  parallelism: { contractDigest },
};
const runtimeConfig = runtimeConfiguration(bindings);

function decision(result = 4): RuntimeDecision<number> {
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

function expectContractEvent(record: LogRecord): void {
  const valid = telemetryEventValidator({
    eventName: record.eventName,
    timeUnixNano: "1790802000000000000",
    attributes: record.attributes,
  });
  expect(
    valid,
    JSON.stringify(telemetryEventValidator.errors, null, 2),
  ).toBe(true);
}

describe("runtime telemetry", () => {
  it("emits decision-received telemetry after a successful decide response", async () => {
    const telemetry = captureLogger();
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(decision()));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
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
      "flaggo.correlation.queuePressure": 0.75,
    });
    expect(record.attributes?.["flaggo.decision.id"]).toEqual(expect.any(String));
    expect(record.attributes?.["flaggo.result.hash"]).toMatch(/^sha256:[0-9a-f]{64}$/u);
    expectContractEvent(record);
  });

  it("keeps multiple decisions individually identifiable", async () => {
    const telemetry = captureLogger();
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(jsonResponse(decision(4)))
      .mockResolvedValueOnce(jsonResponse(decision(8)));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
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
      runtimeConfig,
      fetch,
      telemetry: { logger: telemetry.logger },
    });

    await expect(client.decide("parallelism")).rejects.toThrow();

    expect(telemetry.records).toEqual([]);
  });

  it("rejects non-finite direct-helper correlation values", () => {
    const telemetry = captureLogger();

    for (const value of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      expect(() => recordDecisionReceived(telemetry.logger, {
        contractName: "parallelism",
        decision: decision(),
        correlation: { queuePressure: value },
      })).toThrow(/must be finite/u);
    }

    expect(telemetry.records).toEqual([]);
  });
});

function expectSingle<T>(values: readonly T[]): T {
  expect(values).toHaveLength(1);
  return values[0]!;
}
