import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import * as ts from "typescript";
import { describe, expect, it, vi } from "vitest";

import {
  createFlaggoClient, confirmedExposureAttributes, FlaggoHttpError, InvalidDecisionInputError,
  InvalidServerResponseError, MissingAcceptedDefinitionError,
  type FetchLike, type ProblemDetails, type RegistrationReceipt,
} from "../src/index.js";
import { SDK_VERSION } from "../src/package-version.js";
import { compiled, manifest, response, serverDecision } from "./support.js";

function setup(fallback = false, input = manifest()) {
  const artifacts = compiled(input);
  const fetch = vi.fn<FetchLike>();
  const client = createFlaggoClient({
    catalog: artifacts.catalog, receipt: artifacts.receipt,
    dataPlaneUrl: "https://data.flaggo.test", fetch,
    availabilityFallback: { mode: fallback ? "local-default" : "disabled", retries: 0 },
  });
  return { ...artifacts, client, fetch };
}

function problem(status = 503, eligible = true): ProblemDetails {
  return {
    type: "https://flaggo.dev/problems/dependency-unavailable", status,
    code: "dependency-unavailable", clientFallback: { eligible },
  };
}

function networkFailure(code: string): TypeError {
  return new TypeError("fetch failed", { cause: { code } });
}

describe("manifest-bound runtime client", () => {
  it("initializes synchronously without publication, OTel, or network access", async () => {
    const { client, fetch, receipt } = setup();
    expect(client).not.toBeInstanceOf(Promise);
    expect(fetch).not.toHaveBeenCalled();
    fetch.mockResolvedValue(response(200, serverDecision(receipt)));
    const value = await client.tune.number("parallelism");
    expect(value).toEqual({
      source: "server", value: 4, decisionId: "decision-1",
      exposure: { confirmationRequired: true, confirmToken: "confirmation-1" },
    });
    const body = JSON.parse(String(fetch.mock.calls[0]![1]?.body));
    expect(body).toMatchObject({
      expectedContract: receipt.acceptedDefinitions.parallelism,
      runtimeContext: {}, client: { appId: "worker", environment: "test", sdk: "typescript", sdkVersion: SDK_VERSION },
    });
    expect(body).not.toHaveProperty("definition");
    expect(fetch.mock.calls[0]![0]).toBe("https://data.flaggo.test/v1/decisions/parallelism:decide");
  });

  it("keeps the complete core import graph free of Node, compiler, management, and telemetry dependencies", () => {
    const visited = new Set<string>();
    function visit(path: string): void {
      if (visited.has(path)) return;
      visited.add(path);
      const source = ts.createSourceFile(path, readFileSync(path, "utf8"), ts.ScriptTarget.ES2022, true);
      for (const node of source.statements) {
        if (!ts.isImportDeclaration(node) && !ts.isExportDeclaration(node)) continue;
        if (ts.isImportDeclaration(node) ? node.importClause?.isTypeOnly : node.isTypeOnly) continue;
        const specifier = node.moduleSpecifier;
        if (specifier === undefined || !ts.isStringLiteral(specifier)) continue;
        expect(specifier.text).not.toMatch(/^(node:|typescript$|@opentelemetry\/)/u);
        expect(specifier.text).not.toMatch(/(?:manifest|management|canonical)\.js$/u);
        if (specifier.text.startsWith(".")) {
          visit(resolve(dirname(path), specifier.text.replace(/\.js$/u, ".ts")));
        }
      }
    }
    visit(resolve(import.meta.dirname, "../src/index.ts"));
    expect(visited.size).toBeGreaterThan(3);
    const metadata = JSON.parse(readFileSync(resolve(import.meta.dirname, "../package.json"), "utf8"));
    expect(SDK_VERSION).toBe(metadata.version);
  });

  it.each(["application", "environment", "bundleDigest", "key-set", "contractDigest"])(
    "rejects a mismatched approved %s before transport", (part) => {
      const { catalog, receipt } = compiled();
      if (part === "application") receipt.application = "other";
      if (part === "environment") receipt.environment = "prod";
      if (part === "bundleDigest") receipt.bundleDigest = `sha256:${"0".repeat(64)}`;
      if (part === "key-set") delete receipt.acceptedDefinitions.parallelism;
      if (part === "contractDigest") receipt.acceptedDefinitions.parallelism!.contractDigest = `sha256:${"0".repeat(64)}`;
      expect(() => createFlaggoClient({ catalog, receipt, dataPlaneUrl: "https://data.test" }))
        .toThrow(InvalidServerResponseError);
    },
  );

  it.each([
    { buildId: 1 }, { status: "pending" }, { unexpected: true },
    { issues: [{ code: "invalid", severity: "error", path: "/", message: "rejected" }] },
    { acceptedDefinitions: { parallelism: { definitionId: "", revision: "r", contractDigest: "not-a-digest" } } },
  ])("rejects malformed receipt fields: %o", (override) => {
    const { catalog, receipt } = compiled();
    const config = { catalog, receipt: { ...receipt, ...override }, dataPlaneUrl: "https://data.test" };
    expect(() => Reflect.apply(createFlaggoClient, undefined, [config])).toThrow(InvalidServerResponseError);
  });

  it("captures an immutable accepted identity rather than observing caller mutation", async () => {
    const { catalog, receipt } = compiled();
    const original = structuredClone(receipt);
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, serverDecision(original)));
    const client = createFlaggoClient({ catalog, receipt, dataPlaneUrl: "https://data.test", fetch });
    receipt.acceptedDefinitions.parallelism!.revision = "different";
    await client.tune.number("parallelism");
    expect(JSON.parse(String(fetch.mock.calls[0]![1]?.body)).expectedContract.revision).toBe("revision-1");
    await expect(client.tune.number("unknown")).rejects.toBeInstanceOf(MissingAcceptedDefinitionError);
  });

  it.each([
    undefined, {}, { inputs: {} }, { inputs: { occupancy: "high" } },
    { inputs: { occupancy: -1 } }, { inputs: { occupancy: 2 } },
    { inputs: { occupancy: 0.5, unknown: 1 } },
    { inputs: [{ signal: { key: "occupancy" }, value: 0.5 }] },
    { definition: {}, inputs: { occupancy: 0.5 } },
    { context: null, inputs: { occupancy: 0.5 } },
  ])("rejects invalid caller data before fallback or transport: %o", async (request) => {
    const input = manifest();
    input.decisions.parallelism!.inputs = {
      occupancy: { source: "request", type: "number", range: [0, 1], meaning: "Current occupancy." },
    };
    const { client, fetch } = setup(true, input);
    await expect(Reflect.apply(client.tune.number, undefined, ["parallelism", request]))
      .rejects.toBeInstanceOf(InvalidDecisionInputError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("validates required context and rejects evidence overrides", async () => {
    const input = manifest();
    input.decisions.parallelism!.context = { sessionId: { type: "string", required: true, target: "session" } };
    input.decisions.parallelism!.targeting = { hierarchy: ["session", "global"], primary: "session", fallbackOrder: ["global"] };
    const { client, fetch } = setup(true, input);
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(InvalidDecisionInputError);
    await expect(client.tune.number("parallelism", { context: { sessionId: "" } })).rejects.toBeInstanceOf(InvalidDecisionInputError);
    await expect(client.tune.number("parallelism", { context: { sessionId: "s" }, inputs: { occupancy: 0.5 } }))
      .rejects.toMatchObject({ issues: [{ code: "input-source-conflict" }] });
    expect(fetch).not.toHaveBeenCalled();
  });

  it("sends plain live values with dedicated identity headers and bearer credentials", async () => {
    const input = manifest();
    input.decisions.parallelism!.inputs = {
      occupancy: { source: "request", type: "number", range: [0, 1], meaning: "Current occupancy." },
    };
    const { catalog, receipt } = compiled(input);
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, serverDecision(receipt)));
    const client = createFlaggoClient({
      catalog, receipt, dataPlaneUrl: "https://data.test", fetch,
      dataPlaneCredential: { mode: "bearer", async getToken() { return "test-token"; } },
    });
    await client.tune.number("parallelism", { inputs: { occupancy: 0 }, idempotencyKey: "retry-1", correlationId: "trace-1" });
    const request = fetch.mock.calls[0]![1]!;
    const headers = new Headers(request.headers);
    expect(headers.get("Authorization")).toBe("Bearer test-token");
    expect(headers.get("Idempotency-Key")).toBe("retry-1");
    expect(headers.get("X-Flaggo-Correlation-Id")).toBe("trace-1");
    const body = JSON.parse(String(request.body));
    expect(body.inputs).toEqual({ occupancy: 0 });
    expect(body).not.toHaveProperty("correlationId");
    expect(body).not.toHaveProperty("idempotencyKey");
  });

  it.each([
    { definitionStatus: {} }, { confidence: { evidenceQuality: 1.5 } },
    { exposure: { confirmationRequired: false, confirmToken: "forbidden" } },
    { strategyId: undefined }, { value: null }, { value: 11 }, { value: 0 },
    { auditId: "" }, { unexpected: true },
    { decisionMode: "fallback" }, { targetProvenance: [{ source: "invented" }] },
  ])("rejects incomplete, out-of-contract or mixed-union results: %o", async (override) => {
    const { client, fetch, receipt } = setup(true);
    fetch.mockResolvedValue(response(200, { ...serverDecision(receipt), ...override }));
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});

describe("availability fallback remains narrow", () => {
  it.each([502, 503, 504, 507, 599])("allows an explicitly eligible HTTP %i only when configured", async (status) => {
    const enabled = setup(true);
    enabled.fetch.mockResolvedValue(response(status, problem(status), "application/problem+json"));
    const result = await enabled.client.tune.numberDetailed("parallelism");
    expect(result).toMatchObject({ source: "client-fallback", value: 2, confidence: null });
    expect(result).not.toHaveProperty("decisionId");
    expect(result).not.toHaveProperty("exposure");
    const disabled = setup();
    disabled.fetch.mockResolvedValue(response(status, problem(status), "application/problem+json"));
    await expect(disabled.client.tune.number("parallelism")).rejects.toBeInstanceOf(FlaggoHttpError);
  });

  it.each([400, 401, 403, 404, 409, 422, 429, 500, 501, 505])("never falls back for HTTP %i", async (status) => {
    const { client, fetch } = setup(true);
    fetch.mockResolvedValue(response(status, problem(status), "application/problem+json"));
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(FlaggoHttpError);
  });

  it.each([503, 504])("does not turn required missing evidence or forbidden HTTP %i into success", async (status) => {
    const { client, fetch } = setup(true);
    fetch.mockResolvedValue(response(status, {
      ...problem(status, false), code: "required-evidence-unavailable",
    }, "application/problem+json"));
    await expect(client.tune.number("parallelism")).rejects.toMatchObject({ problem: { code: "required-evidence-unavailable" } });
    expect(fetch).toHaveBeenCalledOnce();
  });

  it.each([
    { status: 502 }, { type: "bad uri" }, { clientFallback: { eligible: "yes" } },
    { issues: [{ code: "BadCode", severity: "error", path: "/", message: "bad" }] },
    { retryAfterSeconds: -1 },
  ])("rejects malformed Problem Details rather than fabricating fallback: %o", async (override) => {
    const { client, fetch } = setup(true);
    fetch.mockResolvedValue(response(503, { ...problem(), ...override }, "application/problem+json"));
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it.each([502, 504])("handles intermediary HTTP %i without confusing an ineligible Flaggo problem", async (status) => {
    const { client, fetch } = setup(true);
    fetch.mockResolvedValue(new Response("gateway unavailable", { status }));
    expect(await client.tune.number("parallelism")).toMatchObject({ source: "client-fallback", value: 2 });
    fetch.mockResolvedValue(response(status, { type: "about:blank", status }, "application/problem+json"));
    expect(await client.tune.number("parallelism")).toMatchObject({ source: "client-fallback" });
    fetch.mockResolvedValue(response(status, { status: 503 }, "application/problem+json"));
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it.each([
    new DOMException("cancelled", "AbortError"),
    networkFailure("CERT_HAS_EXPIRED"),
    new TypeError("unknown failure"),
    new Error("outer", { cause: new DOMException("cancelled", "AbortError") }),
    new DOMException("bad configuration", "InvalidStateError"),
    Object.assign(new Error("retryable outer"), { code: "ECONNRESET", cause: { code: "CERT_HAS_EXPIRED" } }),
  ])("never hides cancellation, TLS, configuration, or unknown transport failures", async (failure) => {
    const { client, fetch } = setup(true);
    fetch.mockRejectedValue(failure);
    await expect(client.tune.number("parallelism")).rejects.toBe(failure);
  });

  it("requires opt-in even for retryable network failures", async () => {
    const enabled = setup(true);
    enabled.fetch.mockRejectedValue(networkFailure("ECONNRESET"));
    expect(await enabled.client.tune.number("parallelism")).toMatchObject({ source: "client-fallback" });
    const disabled = setup();
    const failure = networkFailure("ECONNRESET");
    disabled.fetch.mockRejectedValue(failure);
    await expect(disabled.client.tune.number("parallelism")).rejects.toBe(failure);
  });

  it.each(["network", "body", "503", "504", "in-progress"])("retains identical request identity across %s retries", async (failure) => {
    const { catalog, receipt } = compiled();
    const fetch = vi.fn<FetchLike>();
    if (failure === "network") fetch.mockRejectedValueOnce(networkFailure("ECONNRESET"));
    if (failure === "body") {
      const failed = response(200, {});
      vi.spyOn(failed, "json").mockRejectedValueOnce(networkFailure("ECONNRESET"));
      fetch.mockResolvedValueOnce(failed);
    }
    if (failure === "503") fetch.mockResolvedValueOnce(response(503, problem()));
    if (failure === "504") fetch.mockResolvedValueOnce(new Response("gateway", { status: 504 }));
    if (failure === "in-progress") fetch.mockResolvedValueOnce(response(409, { ...problem(409, false), code: "idempotency-in-progress" }));
    fetch.mockResolvedValueOnce(response(200, serverDecision(receipt)));
    const client = createFlaggoClient({
      catalog, receipt, dataPlaneUrl: "https://data.test", fetch,
      availabilityFallback: { mode: "local-default", retries: 1 },
    });
    expect(await client.tune.number("parallelism")).toMatchObject({ source: "server" });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[0]![1]?.body).toBe(fetch.mock.calls[1]![1]?.body);
    const first = new Headers(fetch.mock.calls[0]![1]?.headers).get("Idempotency-Key");
    expect(first).toMatch(/^flaggo-sdk:/u);
    expect(new Headers(fetch.mock.calls[1]![1]?.headers).get("Idempotency-Key")).toBe(first);
  });

  it.each([400, 401, 422, 500, 501, 505])("does not hide a body-read failure for forbidden HTTP %i", async (status) => {
    const { client, fetch } = setup(true);
    const bodyFailure = networkFailure("ECONNRESET");
    const failed = response(status, {});
    vi.spyOn(failed, "json").mockRejectedValueOnce(bodyFailure);
    fetch.mockResolvedValueOnce(failed);
    await expect(client.tune.number("parallelism")).rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});

describe("explicit exposure and native correlation", () => {
  it("confirms only on an explicit call and returns native attributes without a producer", async () => {
    const { client, fetch, receipt } = setup();
    fetch.mockResolvedValueOnce(response(200, serverDecision(receipt)));
    await client.tune.number("parallelism");
    expect(fetch).toHaveBeenCalledOnce();
    fetch.mockResolvedValueOnce(response(200, {
      exposureId: "exposure-1", decisionId: "decision-1", status: "confirmed", confirmedAt: "2026-09-23T12:00:00Z",
    }));
    const confirmation = await client.exposures.confirm("decision-1", "confirmation-1");
    expect(fetch.mock.calls[1]![0]).toBe("https://data.flaggo.test/v1/exposures/decision-1:confirm");
    expect(confirmedExposureAttributes(confirmation)).toEqual({
      "flaggo.exposure.id": "exposure-1", "flaggo.decision.id": "decision-1",
    });
  });

  it.each([
    { status: "pending" }, { exposureId: "" }, { decisionId: "foreign" },
    { confirmedAt: "2026-02-30T12:00:00Z" }, { confirmedAt: "2026-09-23T12:00:00+00:00" },
    { confirmedAt: "2026-09-23T12:00:00Z trailing" }, { extra: true },
  ])("rejects malformed or foreign confirmations: %o", async (override) => {
    const { client, fetch } = setup();
    const value = {
      exposureId: "exposure-1", decisionId: "decision-1", status: "confirmed", confirmedAt: "2026-09-23T12:00:00Z",
      ...override,
    };
    fetch.mockResolvedValueOnce(response(200, value));
    await expect(client.exposures.confirm("decision-1", "confirmation-1")).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("does not treat a decision receipt or pending confirmation as exposure proof", () => {
    expect(() => Reflect.apply(confirmedExposureAttributes, undefined, [{ decisionId: "decision-1" }])).toThrow();
    expect(() => Reflect.apply(confirmedExposureAttributes, undefined, [{
      exposureId: "exposure-1", decisionId: "decision-1", status: "pending", confirmedAt: "2026-09-23T12:00:00Z",
    }])).toThrow();
  });
});
