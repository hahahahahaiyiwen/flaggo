import {
  createInferenceSignalHandle,
  createSignalHandle,
  type TelemetrySink,
} from "@flaggo/sdk";

export function createAdaptiveWorkerSignals(sink: TelemetrySink) {
  const itemEnqueued = createSignalHandle({
    kind: "event",
    key: "demo.itemEnqueued",
    fields: {
      itemId: "string",
      processingMs: "number",
      shouldFail: "boolean",
    },
    units: {
      processingMs: "ms",
    },
  }, sink);
  const itemCompleted = createSignalHandle({
    kind: "event",
    key: "demo.itemCompleted",
    fields: {
      itemId: "string",
      succeeded: "boolean",
    },
  }, sink);
  const queueDepth = createSignalHandle({
    kind: "metric",
    key: "demo.queueDepth",
    type: "number",
    source: "app-emitted",
    range: [0, 1000],
  }, sink);
  const queuePressure = createInferenceSignalHandle({
    kind: "metric",
    key: "demo.queuePressure",
    type: "number",
    source: "app-emitted",
    range: [0, 1],
  }, sink);
  const processingLatencyMs = createSignalHandle({
    kind: "metric",
    key: "demo.processingLatencyMs",
    type: "number",
    source: "app-emitted",
    unit: "ms",
    range: [0, 10000],
  }, sink);

  return {
    itemEnqueued,
    itemCompleted,
    queueDepth,
    queuePressure,
    processingLatencyMs,
  };
}
