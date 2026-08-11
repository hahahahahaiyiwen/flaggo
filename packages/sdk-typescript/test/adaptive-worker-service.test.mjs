import { describe, expect, it } from "vitest";

import {
  waitForServiceShutdown,
} from "../../../examples/adaptive-worker/service-health.mjs";

describe("adaptive-worker service lifecycle", () => {
  it("fails when a managed host becomes unhealthy", async () => {
    const controller = new AbortController();
    let checks = 0;
    const lifecycle = {
      signal: controller.signal,
      assertHealthy() {
        checks += 1;
        if (checks === 2) {
          throw new Error("data plane exited unexpectedly");
        }
      },
    };

    await expect(
      waitForServiceShutdown(lifecycle, {
        pollIntervalMilliseconds: 1,
      }),
    ).rejects.toThrow("data plane exited unexpectedly");
  });

  it("returns when service shutdown is requested", async () => {
    const controller = new AbortController();
    const lifecycle = {
      signal: controller.signal,
      assertHealthy() {},
    };
    controller.abort();

    await expect(waitForServiceShutdown(lifecycle)).resolves.toBeUndefined();
  });
});
