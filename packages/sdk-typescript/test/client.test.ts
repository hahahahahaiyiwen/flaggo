import { describe, expect, it, vi } from "vitest";

import {
  createFlaggoClient,
  InvalidDecisionInputError,
  InvalidServerResponseError,
  MissingContractBindingError,
  type FetchLike,
} from "../src/index.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const contracts = {
  parallelism: { contractDigest },
} as const;

function decision(
  result: unknown = 4,
  evaluation: unknown = { source: "rule", rule: "queue-pressure" },
): object {
  return {
    contractDigest,
    executableDigest,
    result,
    evaluation,
  };
}

function jsonResponse(
  body: unknown,
  status = 200,
  responseHeaders?: HeadersInit,
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": "application/json",
      ...Object.fromEntries(new Headers(responseHeaders)),
    },
  });
}

describe("v3 runtime client", () => {
  it("posts one complete RuntimeInput to the exact contract version", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(decision()));
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test/",
      contracts,
      fetch,
      random: () => 0.25,
    });

    const result = await client.decide<number>("parallelism", {
      attributes: {
        queuePressure: 0.75,
        worker: { id: "worker-1", zones: ["west", "east"] },
      },
      currentExposure: { exposureId: "exposure-previous" },
      correlationId: "correlation-1",
    });

    expect(result).toEqual(decision());
    expect(fetch).toHaveBeenCalledTimes(1);
    const [url, init] = fetch.mock.calls[0]!;
    expect(String(url)).toBe(
      `https://decisions.test/v3/decision-contracts/parallelism/versions/`
      + `${encodeURIComponent(contractDigest)}/decisions`,
    );
    expect(init?.method).toBe("POST");
    expect(new Headers(init?.headers).get("X-Flaggo-Correlation-Id"))
      .toBe("correlation-1");
    expect(JSON.parse(String(init?.body))).toEqual({
      attributes: {
        queuePressure: 0.75,
        worker: { id: "worker-1", zones: ["west", "east"] },
        _random: 0.25,
      },
      currentExposure: { exposureId: "exposure-previous" },
    });
  });

  it("reuses the complete input, including _random, for retries", async () => {
    const random = vi.fn(() => 0.375);
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(jsonResponse({
        type: "https://flaggo.dev/problems/dependency-unavailable",
        title: "Dependency unavailable",
        status: 503,
      }, 503, { "Retry-After": "0" }))
      .mockResolvedValueOnce(jsonResponse(decision(6, { source: "default" })));
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      retries: 1,
      fetch,
      random,
    });

    await expect(client.decide<number>("parallelism", {
      attributes: { queuePressure: 0.5 },
    })).resolves.toMatchObject({ result: 6 });

    expect(random).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[0]![1]?.body).toBe(fetch.mock.calls[1]![1]?.body);
    expect(JSON.parse(String(fetch.mock.calls[1]![1]?.body)))
      .toMatchObject({ attributes: { _random: 0.375 } });
  });

  it("retries transport failures but does not synthesize a fallback", async () => {
    const unavailable = new TypeError("connection refused");
    const fetch = vi.fn<FetchLike>()
      .mockRejectedValueOnce(unavailable)
      .mockRejectedValueOnce(unavailable);
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      retries: 1,
      fetch,
    });

    await expect(client.decide("parallelism")).rejects.toBe(unavailable);
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("surfaces standard Problem Details and does not retry client errors", async () => {
    const problem = {
      type: "https://flaggo.dev/problems/invalid-runtime-input",
      title: "Invalid runtime input",
      status: 400,
      detail: "An attribute failed validation.",
    };
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(
      problem,
      400,
      { "X-Flaggo-Correlation-Id": "correlation-server" },
    ));
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      retries: 2,
      fetch,
    });

    await expect(client.decide("parallelism"))
      .rejects.toMatchObject({
        problem,
        response: { correlationId: "correlation-server" },
      });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("rejects nonstandard Problem Details extensions", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse({
      type: "https://flaggo.dev/problems/no-active-executable",
      status: 503,
      code: "no-active-executable",
      clientFallback: { eligible: true },
    }, 503));
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      fetch,
    });

    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it.each([
    [{ attributes: { _random: 0.1 } }, "/attributes/_random"],
    [{ attributes: { pressure: Number.NaN } }, "/attributes/pressure"],
    [{ currentExposure: { exposureId: "" } }, "/currentExposure/exposureId"],
    [{ correlationId: "invalid\r\nheader" }, "/correlationId"],
  ])("rejects invalid SDK input %#", async (request, path) => {
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      fetch: vi.fn(),
    });

    await expect(client.decide("parallelism", request))
      .rejects.toMatchObject({
        message: expect.stringContaining(path),
      });
  });

  it("rejects invalid random sources before sending a request", async () => {
    const fetch = vi.fn<FetchLike>();
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      fetch,
      random: () => 1,
    });

    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidDecisionInputError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("rejects unknown bindings and malformed successful responses", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse({
      ...decision(),
      contractDigest: executableDigest,
    }));
    const client = createFlaggoClient({
      decisionServiceUrl: "https://decisions.test",
      contracts,
      fetch,
    });

    await expect(client.decide("missing" as "parallelism"))
      .rejects.toBeInstanceOf(MissingContractBindingError);
    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});
