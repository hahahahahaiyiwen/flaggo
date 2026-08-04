import { describe, it } from "vitest";

import {
  createDerivedMetricHandle,
  createInferenceSignalHandle,
  createSignalHandle,
  type DerivedMetricHandle,
  type InferenceSignalHandle,
  type SignalHandle,
  type TelemetrySink,
} from "../src/index.js";

const sink: TelemetrySink = { emit() {} };

describe("signal declaration type inference", () => {
  it("derives producer and inference value types from declarations", () => {
    const event = createSignalHandle({
      kind: "event",
      key: "tetris.piecePlaced",
      fields: {
        placementTimeMs: "number",
        hardDrop: "boolean",
      },
    }, sink);
    const inference = createInferenceSignalHandle({
      kind: "metric",
      key: "tetris.boardPressure",
      type: "number",
      source: "app-emitted",
    }, sink);
    const derived = createDerivedMetricHandle({
      kind: "metric",
      key: "tetris.difficultyLabel",
      type: "string",
      source: "derived",
      from: [{ key: "tetris.sessionEnded" }],
      aggregation: "latest(label)",
      window: "24h",
    });

    const eventType: SignalHandle<{
      placementTimeMs: number;
      hardDrop: boolean;
    }> = event;
    const inferenceType: InferenceSignalHandle<number> = inference;
    const derivedType: DerivedMetricHandle<string> = derived;
    void [eventType, inferenceType, derivedType];
  });

  it("excludes events from inference handles", () => {
    const eventDeclaration = {
      kind: "event",
      key: "tetris.piecePlaced",
      fields: { hardDrop: "boolean" },
    } as const;
    // @ts-expect-error inference inputs must be app-emitted primitive metrics
    createInferenceSignalHandle(eventDeclaration, sink);
  });
});
