import { describe, expect, it, vi } from "vitest";
import { applyManifest, RequiresApprovalError } from "../src/management.js";
import { InvalidServerResponseError } from "../src/errors.js";
import type { FetchLike } from "../src/types.js";
import { compiled, response } from "./support.js";

describe("explicit management publication", () => {
  it("applies the sole normalized manifest and verifies the exact receipt", async () => {
    const { bundle, receipt } = compiled();
    const fetch = vi.fn<FetchLike>().mockImplementation(async () => response(200, receipt));
    const config = { bundle, controlPlaneUrl: "https://control.test", credential: { mode: "local-development" as const }, fetch };
    expect(await applyManifest(config)).toEqual(receipt);
    await applyManifest(config);
    expect(fetch.mock.calls[0]![0]).toBe("https://control.test/v1/definition-bundles:apply");
    expect(JSON.parse(String(fetch.mock.calls[0]![1]?.body))).toEqual(bundle);
    expect(new Headers(fetch.mock.calls[0]![1]?.headers).get("Idempotency-Key"))
      .toBe(new Headers(fetch.mock.calls[1]![1]?.headers).get("Idempotency-Key"));
  });

  it("surfaces approval as a separate action and never approves automatically", async () => {
    const { bundle, receipt } = compiled();
    const pending = {
      status: "requires-approval", approvalRequestId: "approval-1",
      application: receipt.application, environment: receipt.environment, bundleDigest: receipt.bundleDigest,
      compatibility: "new-contract-required", expiresAt: "2026-09-30T12:00:00Z", snapshotUrl: "/approval/snapshot",
      changes: [{ kind: "created", decisionKey: "parallelism" }], issues: [],
    };
    const fetch = vi.fn<FetchLike>().mockResolvedValueOnce(response(202, pending));
    await expect(applyManifest({
      bundle, controlPlaneUrl: "https://control.test", credential: { mode: "local-development" }, fetch,
    })).rejects.toBeInstanceOf(RequiresApprovalError);
    expect(fetch).toHaveBeenCalledOnce();
    fetch.mockResolvedValueOnce(response(202, { ...pending, environment: "foreign" }));
    await expect(applyManifest({
      bundle, controlPlaneUrl: "https://control.test", credential: { mode: "local-development" }, fetch,
    })).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it.each(["application", "environment", "bundleDigest", "acceptedDefinitions"])("rejects mismatched %s", async (field) => {
    const { bundle, receipt } = compiled();
    const invalid = { ...receipt, [field]: field === "acceptedDefinitions" ? {} : "incorrect" };
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, invalid));
    await expect(applyManifest({
      bundle, controlPlaneUrl: "https://control.test", credential: { mode: "local-development" }, fetch,
    })).rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});
