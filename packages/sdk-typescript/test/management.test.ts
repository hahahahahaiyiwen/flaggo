import { describe, expect, it, vi } from "vitest";

import {
  createContractServiceClient,
  FlaggoHttpError,
  InvalidDecisionInputError,
  InvalidServerResponseError,
  type DecisionContract,
  type FetchLike,
} from "../src/index.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const contract = {
  name: "worker.batchSize",
  expression_syntax: "flaggo.cel/v1",
  attributes: [
    {
      name: "pressure",
      schema: { type: "number", minimum: 0, maximum: 1 },
    },
  ],
  result: {
    schema: { type: "integer", minimum: 1, maximum: 10 },
    default: 3,
  },
} as const satisfies DecisionContract<number>;

const version = {
  name: contract.name,
  contractDigest,
  status: "ready",
  acceptedAt: "2026-08-01T12:00:00Z",
  activeExecutableDigest: executableDigest,
  contract,
} as const;

function response(
  status: number,
  body: unknown,
  contentType = status >= 400
    ? "application/problem+json"
    : "application/json",
  extraHeaders?: HeadersInit,
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": contentType,
      ...Object.fromEntries(new Headers(extraHeaders)),
    },
  });
}

describe("v3 Contract Service client", () => {
  it("validates a complete contract without publishing it", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(200, {
      status: "valid",
      contractDigest,
      issues: [],
    }));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test/",
      credential: { mode: "local-development" },
      fetch,
    });

    await expect(client.validate(contract.name, contract, {
      correlationId: "validation-1",
    })).resolves.toEqual({
      status: "valid",
      contractDigest,
      issues: [],
    });

    const [url, init] = fetch.mock.calls[0]!;
    expect(String(url)).toBe(
      "https://contracts.test/v3/decision-contracts/worker.batchSize/validate",
    );
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toEqual(contract);
    expect(new Headers(init?.headers).get("X-Flaggo-Correlation-Id"))
      .toBe("validation-1");
  });

  it("publishes and validates the ready version envelope", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(201, version, undefined, {
      Location: `/v3/decision-contracts/${contract.name}/versions/${contractDigest}`,
    }));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.put<number>(contract.name, contract)).resolves.toEqual(version);
    expect(fetch.mock.calls[0]![1]?.method).toBe("PUT");
  });

  it("reads current, exact, and paginated versions", async () => {
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, version))
      .mockResolvedValueOnce(response(200, version))
      .mockResolvedValueOnce(response(200, {
        name: contract.name,
        currentContractDigest: contractDigest,
        versions: [{
          contractDigest,
          status: "ready",
          acceptedAt: version.acceptedAt,
          activeExecutableDigest: executableDigest,
        }],
        nextCursor: null,
      }));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.getCurrent<number>(contract.name)).resolves.toEqual(version);
    await expect(client.getVersion<number>(contract.name, contractDigest))
      .resolves.toEqual(version);
    await expect(client.listVersions(contract.name, {
      limit: 10,
      cursor: "next/page",
    })).resolves.toMatchObject({ currentContractDigest: contractDigest });

    expect(String(fetch.mock.calls[1]![0])).toContain(
      `/versions/${encodeURIComponent(contractDigest)}`,
    );
    expect(String(fetch.mock.calls[2]![0])).toMatch(
      /\/versions\?limit=10&cursor=next%2Fpage$/u,
    );
  });

  it("surfaces standard Problem Details and response headers", async () => {
    const problem = {
      type: "https://flaggo.dev/problems/invalid-decision-contract",
      title: "Invalid decision contract",
      status: 422,
      detail: "The decision contract failed semantic validation.",
    };
    const fetch = vi.fn<FetchLike>(async () => response(
      422,
      problem,
      undefined,
      {
        "X-Flaggo-Correlation-Id": "server-correlation",
        "Retry-After": "1",
      },
    ));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.put(contract.name, contract)).rejects.toMatchObject({
      problem,
      response: {
        correlationId: "server-correlation",
        retryAfterSeconds: 1,
      },
    });
  });

  it("rejects invalid identities and malformed service responses", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(200, {
      ...version,
      contractDigest: executableDigest,
    }));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.validate("other.name", contract))
      .rejects.toBeInstanceOf(InvalidDecisionInputError);
    await expect(client.getVersion(contract.name, "sha256:bad"))
      .rejects.toBeInstanceOf(InvalidDecisionInputError);
    await expect(client.getVersion(contract.name, contractDigest))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects nonstandard Problem Details extensions", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(404, {
      type: "https://flaggo.dev/problems/contract-version-not-found",
      status: 404,
      code: "not-found",
    }));
    const client = createContractServiceClient({
      contractServiceUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.getCurrent(contract.name))
      .rejects.not.toBeInstanceOf(FlaggoHttpError);
    await expect(client.getCurrent(contract.name))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});
