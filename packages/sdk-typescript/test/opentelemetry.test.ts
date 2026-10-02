import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

import { ExportResultCode } from "@opentelemetry/core";
import { resourceFromAttributes } from "@opentelemetry/resources";
import { LoggerProvider } from "@opentelemetry/sdk-logs";
import { MeterProvider } from "@opentelemetry/sdk-metrics";
import {
  type ReadableSpan,
  TracerProvider,
} from "@opentelemetry/sdk-trace";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  createFlaggoResource,
  createFlaggoLogRecordProcessor,
  createFlaggoMetricReader,
  createFlaggoSpanProcessor,
  createFlaggoTraceExporter,
  type FlaggoMetricSelector,
} from "../src/opentelemetry/index.js";
import { runtimeConfiguration } from "./runtime-configuration.js";

interface ReceivedRequest {
  readonly contentType: string | undefined;
  readonly path: string | undefined;
  readonly payload: Record<string, unknown>;
}

const servers: Server[] = [];
const testDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const testBindings = { test: { contractDigest: testDigest } } as const;

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) =>
    new Promise<void>((resolve, reject) => {
      server.close((error) => error === undefined ? resolve() : reject(error));
    })
  ));
});

describe("OpenTelemetry integration", () => {
  it("exports all signals by default and applies optional selectors", async () => {
    const received: ReceivedRequest[] = [];
    const server = createServer((request, response) => {
      const chunks: Buffer[] = [];
      request.on("data", (chunk: Buffer) => chunks.push(chunk));
      request.on("end", () => {
        received.push({
          contentType: request.headers["content-type"],
          path: request.url,
          payload: JSON.parse(Buffer.concat(chunks).toString("utf8")) as
            Record<string, unknown>,
        });
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end("{}");
      });
    });
    servers.push(server);
    await new Promise<void>((resolve) =>
      server.listen(0, "127.0.0.1", resolve)
    );
    const address = server.address() as AddressInfo;
    const defaultBaseUrl = `http://127.0.0.1:${address.port}/default`;
    const selectedBaseUrl = `http://127.0.0.1:${address.port}/selected`;
    const authority = {
      tenant: "local",
      application: "otel-helper-test",
      environment: "test",
    } as const;
    const defaultRuntimeConfig = runtimeConfiguration(testBindings, {
      authority,
      otlpIngestionUrl: defaultBaseUrl,
    });
    const selectedRuntimeConfig = runtimeConfiguration(testBindings, {
      authority,
      otlpIngestionUrl: selectedBaseUrl,
    });
    const resource = createFlaggoResource({
      baseResource: resourceFromAttributes({
        "service.name": "otel-helper-test",
      }),
      runtimeConfig: defaultRuntimeConfig,
    });
    const logSelector = vi.fn((record) =>
      record.eventName?.startsWith("keep.") === true
    );
    const spanSelector = vi.fn((span) => span.name.startsWith("keep."));
    const metricSelector = vi.fn<FlaggoMetricSelector>((metric) =>
      metric.descriptor.name.startsWith("keep.")
    );
    const loggerProvider = new LoggerProvider({
      resource,
      processors: [
        createFlaggoLogRecordProcessor({
          runtimeConfig: defaultRuntimeConfig,
          batch: { scheduledDelayMillis: 60_000 },
        }),
        createFlaggoLogRecordProcessor({
          runtimeConfig: selectedRuntimeConfig,
          shouldExport: logSelector,
          batch: { scheduledDelayMillis: 60_000 },
        }),
      ],
    });
    const tracerProvider = new TracerProvider({
      resource,
      spanProcessors: [
        createFlaggoSpanProcessor({
          runtimeConfig: defaultRuntimeConfig,
          batch: { scheduledDelayMillis: 60_000 },
        }),
        createFlaggoSpanProcessor({
          runtimeConfig: selectedRuntimeConfig,
          shouldExport: spanSelector,
          batch: { scheduledDelayMillis: 60_000 },
        }),
      ],
    });
    const meterProvider = new MeterProvider({
      resource,
      readers: [
        createFlaggoMetricReader({
          runtimeConfig: defaultRuntimeConfig,
          periodic: { exportIntervalMillis: 60_000 },
        }),
        createFlaggoMetricReader({
          runtimeConfig: selectedRuntimeConfig,
          shouldExport: metricSelector,
          periodic: { exportIntervalMillis: 60_000 },
        }),
      ],
    });

    try {
      const logger = loggerProvider.getLogger("test.logs");
      logger.emit({ eventName: "keep.log", attributes: { selected: true } });
      logger.emit({ eventName: "drop.log", attributes: { selected: false } });

      tracerProvider.getTracer("test.traces")
        .startSpan("keep.span")
        .end();
      tracerProvider.getTracer("test.traces")
        .startSpan("drop.span")
        .end();

      const meter = meterProvider.getMeter("test.metrics");
      meter.createGauge("keep.metric").record(7);
      meter.createGauge("drop.metric").record(9);

      await Promise.all([
        loggerProvider.forceFlush(),
        tracerProvider.forceFlush(),
        meterProvider.forceFlush(),
      ]);

      expect(received).toHaveLength(6);
      expect(received.map((request) => request.path).sort()).toEqual([
        "/default/v1/logs",
        "/default/v1/metrics",
        "/default/v1/traces",
        "/selected/v1/logs",
        "/selected/v1/metrics",
        "/selected/v1/traces",
      ]);
      expect(received.every((request) =>
        request.contentType === "application/json"
      )).toBe(true);
      expect(JSON.stringify(received)).toContain('"flaggo.tenant"');
      expect(JSON.stringify(received)).toContain('"flaggo.application"');
      expect(JSON.stringify(received)).toContain('"flaggo.environment"');

      const logs = requestFor(received, "/selected/v1/logs").payload;
      expect(JSON.stringify(logs)).toContain("keep.log");
      expect(JSON.stringify(logs)).not.toContain("drop.log");

      const traces = requestFor(received, "/selected/v1/traces").payload;
      expect(JSON.stringify(traces)).toContain("keep.span");
      expect(JSON.stringify(traces)).not.toContain("drop.span");

      const metrics = requestFor(received, "/selected/v1/metrics").payload;
      expect(JSON.stringify(metrics)).toContain("keep.metric");
      expect(JSON.stringify(metrics)).not.toContain("drop.metric");

      expect(JSON.stringify(
        requestFor(received, "/default/v1/logs").payload,
      )).toContain("drop.log");
      expect(JSON.stringify(
        requestFor(received, "/default/v1/traces").payload,
      )).toContain("drop.span");
      expect(JSON.stringify(
        requestFor(received, "/default/v1/metrics").payload,
      )).toContain("drop.metric");

      expect(logSelector).toHaveBeenCalledTimes(2);
      expect(spanSelector).toHaveBeenCalledTimes(2);
      expect(metricSelector).toHaveBeenCalledTimes(2);
      expect(logSelector.mock.calls[0]?.[0]).toMatchObject({
        instrumentationScope: { name: "test.logs" },
      });
      expect(
        logSelector.mock.calls[0]?.[0].resource.attributes["service.name"],
      ).toBe("otel-helper-test");
      expect(
        logSelector.mock.calls[0]?.[0].resource.attributes["flaggo.tenant"],
      ).toBe("local");
      expect(spanSelector.mock.calls[0]?.[0]).toMatchObject({
        instrumentationScope: { name: "test.traces" },
      });
      expect(metricSelector.mock.calls[0]?.[1]).toMatchObject({
        instrumentationScope: { name: "test.metrics" },
      });
      expect(
        metricSelector.mock.calls[0]?.[1].resource.attributes["service.name"],
      ).toBe("otel-helper-test");
    } finally {
      await Promise.all([
        loggerProvider.shutdown(),
        tracerProvider.shutdown(),
        meterProvider.shutdown(),
      ]);
    }
  });

  it.each([
    ["relative", "Flaggo OTLP baseUrl must be an absolute URL."],
    ["ftp://telemetry.test", "Flaggo OTLP baseUrl must use HTTP or HTTPS."],
    [
      "https://user:secret@telemetry.test",
      "Flaggo OTLP baseUrl must not contain credentials.",
    ],
    [
      "https://telemetry.test?tenant=one",
      "Flaggo OTLP baseUrl must not contain a query string or fragment.",
    ],
  ])("rejects invalid OTLP URL %s", (otlpIngestionUrl) => {
    expect(() =>
      runtimeConfiguration(testBindings, { otlpIngestionUrl })
    ).toThrow();
  });

  it("rejects a base Resource with conflicting Flaggo authority", () => {
    const runtimeConfig = runtimeConfiguration(testBindings);
    const baseResource = resourceFromAttributes({
      "flaggo.tenant": "other",
    });

    expect(() => createFlaggoResource({ baseResource, runtimeConfig }))
      .toThrow(
        "OpenTelemetry Resource attribute 'flaggo.tenant' must equal 'local'",
      );
  });

  it("does not export telemetry from a mismatched Resource", async () => {
    const runtimeConfig = runtimeConfiguration(testBindings);
    const exporter = createFlaggoTraceExporter({ runtimeConfig });
    const span = {
      resource: resourceFromAttributes({
        "flaggo.tenant": "other",
        "flaggo.application": "sdk-test",
        "flaggo.environment": "test",
      }),
    } as ReadableSpan;

    const result = await new Promise<{
      readonly code: ExportResultCode;
      readonly error?: Error;
    }>((resolve) => exporter.export([span], resolve));

    expect(result.code).toBe(ExportResultCode.FAILED);
    expect(result.error?.message).toContain(
      "OpenTelemetry Resource attribute 'flaggo.tenant' must equal 'local'",
    );
    await exporter.shutdown();
  });
});

function requestFor(
  requests: readonly ReceivedRequest[],
  path: string,
): ReceivedRequest {
  const request = requests.find((candidate) => candidate.path === path);
  expect(request).toBeDefined();
  return request!;
}
