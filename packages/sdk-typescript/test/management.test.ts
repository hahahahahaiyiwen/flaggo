import { describe, expect, it, vi } from "vitest";

import {
  bindDecisionContract,
  createContractClient,
  FlaggoHttpError,
  InvalidFlaggoInputError,
  InvalidServerResponseError,
  type DecisionContract,
  type FetchLike,
} from "../src/management/index.js";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const contractDefinition = {
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
} as const;
const contract = bindDecisionContract(contractDefinition, {
  tenant: "local",
  application: "worker",
  environment: "test",
}) satisfies DecisionContract<number>;

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
  extraHeaders?: HeadersInit,
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": status >= 400
        ? "application/problem+json"
        : "application/json",
      ...Object.fromEntries(new Headers(extraHeaders)),
    },
  });
}

describe("v3 Contract Service client", () => {
  it("validates a complete contract without deploying it", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(200, {
      status: "valid",
      contractDigest,
      issues: [],
    }));
    const client = createContractClient({
      baseUrl: "https://contracts.test/",
      credential: { mode: "local-development" },
      fetch,
    });

    await expect(client.validate(contract, {
      correlationId: "validation-1",
    })).resolves.toEqual({
      value: {
        status: "valid",
        contractDigest,
        issues: [],
      },
      metadata: { status: 200 },
    });

    const [url, init] = fetch.mock.calls[0]!;
    expect(String(url)).toBe(
      "https://contracts.test/v3/decision-contracts/worker.batchSize/validate",
    );
    expect(init?.method).toBe("POST");
    expect(JSON.parse(String(init?.body))).toEqual(contract);
    expect(new Headers(init?.headers).get("X-Flaggo-Correlation-Id"))
      .toBe("validation-1");
    expect(new Headers(init?.headers).get("Authorization"))
      .toBe("Flaggo-Local-Development");
  });

  it("deploys a ready version and exposes response metadata", async () => {
    const location =
      `/v3/decision-contracts/${contract.name}/versions/${contractDigest}`;
    const fetch = vi.fn<FetchLike>(async () => response(201, version, {
      Location: location,
      "X-Flaggo-Correlation-Id": "deployment-1",
    }));
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.deploy(contract)).resolves.toEqual({
      value: version,
      metadata: {
        status: 201,
        location,
        correlationId: "deployment-1",
      },
    });
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
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.getCurrent<number>(contract.name))
      .resolves.toMatchObject({ value: version });
    await expect(client.getVersion<number>(contract.name, contractDigest))
      .resolves.toMatchObject({ value: version });
    await expect(client.listVersions(
      contract.name,
      { limit: 10, cursor: "next/page" },
    )).resolves.toMatchObject({
      value: { currentContractDigest: contractDigest },
    });

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
      {
        "X-Flaggo-Correlation-Id": "server-correlation",
        "Retry-After": "1",
      },
    ));
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.deploy(contract)).rejects.toMatchObject({
      problem,
      response: {
        status: 422,
        correlationId: "server-correlation",
        retryAfterSeconds: 1,
      },
    });
    await expect(client.deploy(contract)).rejects.toBeInstanceOf(FlaggoHttpError);
  });

  it("rejects malformed local contracts before transport", async () => {
    const fetch = vi.fn<FetchLike>();
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.deploy({
      ...contract,
      attributes: [{ name: "_reserved", schema: { type: "number" } }],
    } as never)).rejects.toBeInstanceOf(InvalidFlaggoInputError);
    await expect(client.deploy(contractDefinition as never))
      .rejects.toBeInstanceOf(InvalidFlaggoInputError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("rejects invalid route identities and malformed responses", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(200, {
      ...version,
      contractDigest: executableDigest,
    }));
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.getCurrent("invalid/name"))
      .rejects.toBeInstanceOf(InvalidFlaggoInputError);
    await expect(client.getVersion(contract.name, "sha256:bad"))
      .rejects.toBeInstanceOf(InvalidFlaggoInputError);
    await expect(client.getVersion(contract.name, contractDigest))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects a Problem Details status that conflicts with HTTP", async () => {
    const fetch = vi.fn<FetchLike>(async () => response(404, {
      type: "https://flaggo.dev/problems/contract-version-not-found",
      status: 409,
      traceId: "trace-1",
    }));
    const client = createContractClient({
      baseUrl: "https://contracts.test",
      fetch,
    });

    await expect(client.getCurrent(contract.name))
      .rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});
