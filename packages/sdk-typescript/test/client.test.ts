import { describe, expect, it, vi } from "vitest";

import {
  createDecisionClient,
  FlaggoAbortError,
  FlaggoHttpError,
  FlaggoTimeoutError,
  FlaggoTransportError,
  InvalidFlaggoInputError,
  InvalidServerResponseError,
  MissingDecisionBindingError,
  type DecisionBindings,
  type DecisionSpec,
  type FetchLike,
} from "../src/runtime/index.js";
import { runtimeConfiguration } from "./runtime-configuration.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";

type Decisions = {
  readonly parallelism: DecisionSpec<{
    readonly queuePressure: number;
    readonly worker: {
      readonly id: string;
      readonly zones: readonly string[];
    };
  }, number>;
};

const bindings: DecisionBindings<Decisions> = {
  parallelism: { contractDigest },
};
const runtimeConfig = runtimeConfiguration(bindings);

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
      "Content-Type": status >= 400
        ? "application/problem+json"
        : "application/json",
      ...Object.fromEntries(new Headers(responseHeaders)),
    },
  });
}

describe("v3 runtime client", () => {
  it("posts one complete RuntimeInput to the exact contract version", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(
      decision(),
      200,
      { "X-Flaggo-Correlation-Id": "server-correlation" },
    ));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
      random: () => 0.25,
    });

    const response = await client.decide("parallelism", {
      attributes: {
        queuePressure: 0.75,
        worker: { id: "worker-1", zones: ["west", "east"] },
      },
      currentExposure: { exposureId: "exposure-previous" },
    }, {
      correlationId: "correlation-1",
    });

    expect(response).toEqual({
      value: decision(),
      metadata: {
        status: 200,
        correlationId: "server-correlation",
      },
    });
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

  it("reuses exact request bytes without adding authorization for retries", async () => {
    const random = vi.fn(() => 0.375);
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(jsonResponse({
        type: "https://flaggo.dev/problems/dependency-unavailable",
        title: "Dependency unavailable",
        status: 503,
      }, 503, { "Retry-After": "0" }))
      .mockResolvedValueOnce(jsonResponse(decision(6, { source: "default" })));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      retry: { maxAttempts: 2, baseDelayMs: 0, maxDelayMs: 0 },
      fetch,
      random,
    });

    await expect(client.decide("parallelism", {
      attributes: { queuePressure: 0.5 },
    })).resolves.toMatchObject({ value: { result: 6 } });

    expect(random).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(fetch.mock.calls[0]![1]?.body).toBe(fetch.mock.calls[1]![1]?.body);
    expect(new Headers(fetch.mock.calls[0]![1]?.headers).get("Authorization"))
      .toBeNull();
    expect(new Headers(fetch.mock.calls[1]![1]?.headers).get("Authorization"))
      .toBeNull();
  });

  it("normalizes exhausted transport failures without synthesizing fallback", async () => {
    const unavailable = new TypeError("connection refused");
    const fetch = vi.fn<FetchLike>()
      .mockRejectedValueOnce(unavailable)
      .mockRejectedValueOnce(unavailable);
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      retry: { maxAttempts: 2, baseDelayMs: 0, maxDelayMs: 0 },
      fetch,
    });

    await expect(client.decide("parallelism")).rejects.toMatchObject({
      name: "FlaggoTransportError",
      cause: unavailable,
    });
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("surfaces standard Problem Details without retrying client errors", async () => {
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
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      retry: { maxAttempts: 3 },
      fetch,
    });

    await expect(client.decide("parallelism")).rejects.toMatchObject({
      problem,
      response: {
        status: 400,
        correlationId: "correlation-server",
      },
    });
    await expect(client.decide("parallelism")).rejects.toBeInstanceOf(
      FlaggoHttpError,
    );
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("accepts optional Problem Details members and extensions", async () => {
    const problem = {
      title: "No active executable",
      traceId: "trace-1",
    };
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(problem, 503));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
    });

    const request = client.decide("parallelism");
    await expect(request).rejects.toBeInstanceOf(FlaggoHttpError);
    await expect(request).rejects.toMatchObject({
      problem,
      response: { status: 503 },
      message: "No active executable",
    });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("normalizes response stream failures", async () => {
    const streamFailure = new Error("response stream failed");
    const fetch = vi.fn<FetchLike>(async () => new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.error(streamFailure);
        },
      }),
      {
        status: 200,
        headers: { "Content-Type": "application/json" },
      },
    ));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
    });

    await expect(client.decide("parallelism")).rejects.toMatchObject({
      name: "FlaggoTransportError",
      cause: streamFailure,
    });
  });

  it.each([
    [{ attributes: { _random: 0.1 } }, "/attributes/_random"],
    [{ attributes: { queuePressure: Number.NaN } }, "/attributes/queuePressure"],
    [{ attributes: { worker: new Date() } }, "/attributes/worker"],
    [{ currentExposure: { exposureId: "" } }, "/currentExposure/exposureId"],
  ])("rejects values that do not serialize as valid RuntimeInput %#", async (
    request,
    path,
  ) => {
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch: vi.fn(),
    });

    await expect(client.decide(
      "parallelism",
      request as never,
    )).rejects.toMatchObject({
      name: "InvalidFlaggoInputError",
      message: expect.stringContaining(path),
    });
  });

  it("rejects sparse arrays and invalid random sources before transport", async () => {
    const fetch = vi.fn<FetchLike>();
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
      random: () => 1,
    });
    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidFlaggoInputError);

    const sparse: string[] = [];
    sparse.length = 1;
    const validRandomClient = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
      random: () => 0.5,
    });
    await expect(validRandomClient.decide("parallelism", {
      attributes: {
        worker: { id: "worker", zones: sparse },
      },
    })).rejects.toBeInstanceOf(InvalidFlaggoInputError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("enforces request and response document limits", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse(
      decision(),
      200,
      { "Content-Length": "262145" },
    ));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
    });

    const oversizedAttributes = Object.fromEntries(
      Array.from(
        { length: 17 },
        (_, index) => [`value${index}`, "x".repeat(16_384)],
      ),
    );
    await expect(client.decide("parallelism", {
      attributes: oversizedAttributes,
    } as never)).rejects.toBeInstanceOf(InvalidFlaggoInputError);
    expect(fetch).not.toHaveBeenCalled();

    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects unknown bindings and malformed successful responses", async () => {
    const fetch = vi.fn<FetchLike>(async () => jsonResponse({
      ...decision(),
      contractDigest: executableDigest,
    }));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
    });

    await expect(client.decide("missing" as "parallelism"))
      .rejects.toBeInstanceOf(MissingDecisionBindingError);
    await expect(client.decide("toString" as "parallelism"))
      .rejects.toBeInstanceOf(MissingDecisionBindingError);
    await expect(client.decide("constructor" as "parallelism"))
      .rejects.toBeInstanceOf(MissingDecisionBindingError);
    expect(fetch).not.toHaveBeenCalled();
    await expect(client.decide("parallelism"))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("supports caller cancellation and operation timeouts", async () => {
    const fetch = vi.fn<FetchLike>(() => new Promise(() => {}));
    const client = createDecisionClient<Decisions>({
      runtimeConfig,
      fetch,
    });

    const controller = new AbortController();
    controller.abort();
    await expect(client.decide("parallelism", {}, {
      signal: controller.signal,
    })).rejects.toBeInstanceOf(FlaggoAbortError);

    await expect(client.decide("parallelism", {}, {
      timeoutMs: 10,
    })).rejects.toBeInstanceOf(FlaggoTimeoutError);
  });

  it("exports transport errors through the runtime entry point", () => {
    expect(FlaggoTransportError).toBeDefined();
  });
});
