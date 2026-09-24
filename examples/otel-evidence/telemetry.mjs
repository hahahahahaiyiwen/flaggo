import { resourceFromAttributes } from "@opentelemetry/resources";
import { OTLPMetricExporter } from "@opentelemetry/exporter-metrics-otlp-proto";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-proto";
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-proto";
import { MeterProvider, PeriodicExportingMetricReader } from "@opentelemetry/sdk-metrics";
import { AlwaysOnSampler, SimpleSpanProcessor, TracerProvider } from "@opentelemetry/sdk-trace";
import { InMemoryLogRecordExporter, LoggerProvider, SimpleLogRecordProcessor } from "@opentelemetry/sdk-logs";

export function createApplicationTelemetry(collectorUrl) {
  const resource = resourceFromAttributes({ "service.name": "otel-worker" });
  const metrics = new MeterProvider({
    resource,
    readers: [new PeriodicExportingMetricReader({
      exporter: new OTLPMetricExporter({ url: `${collectorUrl}/v1/metrics`, headers: {} }),
      exportIntervalMillis: 60_000,
    })],
  });
  const traces = new TracerProvider({
    resource,
    sampler: new AlwaysOnSampler(),
    spanProcessors: [new SimpleSpanProcessor({
      exporter: new OTLPTraceExporter({ url: `${collectorUrl}/v1/traces`, headers: {} }),
    })],
  });
  const captured = new InMemoryLogRecordExporter();
  const logs = new LoggerProvider({
    resource,
    processors: [
      new SimpleLogRecordProcessor({
        exporter: new OTLPLogExporter({ url: `${collectorUrl}/v1/logs`, headers: {} }),
      }),
      new SimpleLogRecordProcessor({ exporter: captured }),
    ],
  });
  const gauge = metrics.getMeter("example.worker", "1")
    .createGauge("work.queue.pressure", { unit: "1" });
  const tracer = traces.getTracer("example.worker", "1");
  const logger = logs.getLogger("example.worker", "1");

  return {
    logger,
    captured,
    emitWorkload(sessionId) {
      const attributes = { "session.id": sessionId };
      gauge.record(0.75, attributes);
      const end = Date.now() - 1;
      const span = tracer.startSpan("work.process", { attributes, startTime: end - 250 });
      span.addEvent("work.retry", { "retry.count": 2 }, end - 50);
      span.end(end);
      logger.emit({
        eventName: "work.summary",
        timestamp: end,
        attributes,
        body: { counts: { failures: 2 } },
      });
      return span.spanContext();
    },
    async flush() {
      await Promise.all([metrics.forceFlush(), traces.forceFlush(), logs.forceFlush()]);
    },
    async stop() {
      await Promise.all([metrics.shutdown(), traces.shutdown(), logs.shutdown()]);
    },
  };
}
