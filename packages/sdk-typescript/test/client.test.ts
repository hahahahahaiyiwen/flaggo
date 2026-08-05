import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";

import {
  ContractConflictError,
  FlaggoHttpError,
  InvalidServerResponseError,
  MissingStaticDefinitionError,
  RequiresApprovalError,
  contractDigest,
  createDerivedMetricHandle,
  createInferenceSignalHandle,
  createFlaggoClient,
  createOpenTelemetrySink,
  createSignalHandle,
  normalizeBundle,
  signalSchemaDigest,
  type AcceptedDefinition,
  type DecisionDefinitionBundle as ContractDecisionDefinitionBundle,
  type FetchLike,
  type NumberDecisionDefinition,
  type RegistrationReceipt,
} from "../src/index.js";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const sdkPackage = JSON.parse(
  readFileSync(resolve(import.meta.dirname, "../package.json"), "utf8"),
) as { version: string };

type DecisionDefinitionBundle = Omit<
  ContractDecisionDefinitionBundle,
  "definitions"
> & {
  definitions: NumberDecisionDefinition[];
};

function fixture<T>(path: string): T {
  return JSON.parse(
    readFileSync(resolve(repositoryRoot, "contracts/fixtures", path), "utf8"),
  ) as T;
}

function response(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function networkFailure(code: string): TypeError {
  return new TypeError("fetch failed", { cause: { code } });
}

function decisionForReceipt(
  body: unknown,
  receipt: RegistrationReceipt,
  decisionKey: string,
  deploymentId?: string,
): unknown {
  const result = structuredClone(body) as Record<string, unknown>;
  result.decisionKey = decisionKey;
  result.definitionStatus = {
    ...receipt.acceptedDefinitions[decisionKey],
    bundleDigest: receipt.bundleDigest,
    ...(receipt.buildId === undefined ? {} : { buildId: receipt.buildId }),
    ...(deploymentId === undefined ? {} : { deploymentId }),
    integrity: "verified",
    compatibility: "identical",
  };
  return result;
}

describe("canonical definition identity", () => {
  it("matches the accepted Phase 1 numeric definition digest", () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      digest: { contractDigest: string };
    }>("management/definition-bundle/04-apply-approved-receipt.json");

    expect(contractDigest(apply.request.body.definitions[0]!)).toBe(
      apply.digest.contractDigest,
    );
  });

  it("sorts keys by code point rather than the host locale", () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const upper = structuredClone(apply.request.body.definitions[0]!);
    upper.key = "B";
    const lower = structuredClone(apply.request.body.definitions[0]!);
    lower.key = "a";
    const bundle = structuredClone(apply.request.body);
    bundle.definitions = [lower, upper];

    expect(normalizeBundle(bundle).definitions.map(({ key }) => key)).toEqual([
      "B",
      "a",
    ]);
  });

  it("verifies supplied signal digests and rejects same-key conflicts", () => {
    const declaration = {
      kind: "metric" as const,
      key: "tetris.boardPressure",
      type: "number" as const,
      source: "app-emitted" as const,
      range: [0, 1] as [number, number],
    };
    const digest = signalSchemaDigest(declaration);
    expect(
      createSignalHandle({ ...declaration, schemaDigest: digest }, {
        emit() {},
      }).schemaDigest,
    ).toBe(digest);
    expect(() =>
      createSignalHandle({
        ...declaration,
        schemaDigest: `sha256:${"0".repeat(64)}`,
      }, { emit() {} })
    ).toThrow(/schema digest mismatch/);

    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const bundle = structuredClone(apply.request.body);
    bundle.signals = [
      declaration,
      { ...declaration, range: [0, 2] },
    ];
    expect(() => normalizeBundle(bundle)).toThrow(
      /conflicting signal declaration/,
    );
  });
});

describe("startup registration", () => {
  it("initializes runtime bindings only from acceptedDefinitions", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{
      expected: { body: unknown };
    }>("runtime/decide/02-active-numeric-strategy.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      ));

    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    const result = await client.tune.numberDetailed("tetris.dropInterval", {
      runtimeTarget: { type: "session", id: "game-456" },
      context: {
        sessionId: "game-456",
        userId: "user-123",
        cohort: "new_players",
        deviceType: "mobile",
      },
      inputs: [{ signal: { key: "tetris.boardPressure" }, value: 0.82 }],
    });

    expect(result.source).toBe("server");
    expect(result.value).toBe(700);
    const runtimeBody = JSON.parse(
      String(fetch.mock.calls[1]![1]?.body),
    ) as Record<string, unknown>;
    expect(runtimeBody).not.toHaveProperty("definition");
    expect(runtimeBody).toMatchObject({
      expectedContract: apply.expected.body.acceptedDefinitions["tetris.dropInterval"],
      client: {
        appId: "tetris-demo",
        environment: "dev",
        sdk: "typescript",
        sdkVersion: sdkPackage.version,
      },
    });
  });

  it("fails typed and leaves no client when approval is required", async () => {
    const pending = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: { approvalRequestId: string } };
    }>("management/definition-bundle/06-apply-requires-approval.json");
    const fetch = vi.fn<FetchLike>().mockResolvedValue(
      response(202, pending.expected.body),
    );

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: pending.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        fetch,
      }),
    ).rejects.toEqual(
      expect.objectContaining<Partial<RequiresApprovalError>>({
        name: "RequiresApprovalError",
        approvalRequestId: pending.expected.body.approvalRequestId,
      }),
    );
  });

  it("rejects an approval response for another bundle identity", async () => {
    const pending = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: Record<string, unknown> };
    }>("management/definition-bundle/06-apply-requires-approval.json");
    const invalid = structuredClone(pending.expected.body);
    invalid.application = "another-app";
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(202, invalid));

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: pending.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        fetch,
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects a receipt whose digest does not match the submitted bundle", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const receipt = structuredClone(apply.expected.body);
    receipt.bundleDigest = `sha256:${"0".repeat(64)}`;
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, receipt));

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        fetch,
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects schema-invalid registration receipt fields", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const receipt = structuredClone(apply.expected.body) as RegistrationReceipt
      & Record<string, unknown>;
    receipt.buildId = 42 as unknown as string;
    receipt.unexpected = true;
    const accepted = receipt.acceptedDefinitions["tetris.dropInterval"] as
      AcceptedDefinition & Record<string, unknown>;
    accepted.unexpected = true;
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, receipt));

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        fetch,
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("binds approved receipt identity to the submitted bundle", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const receipt = structuredClone(apply.expected.body);
    receipt.application = "another-app";
    const fetch = vi.fn<FetchLike>().mockResolvedValue(response(200, receipt));

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "another-app",
        environment: "dev",
        fetch,
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("uses deterministic registration identity across concurrent clients", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>().mockImplementation(async () =>
      response(200, apply.expected.body)
    );
    const config = {
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register" as const,
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" as const },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    };

    await Promise.all([createFlaggoClient(config), createFlaggoClient(config)]);

    const keys = fetch.mock.calls.map(([, init]) =>
      new Headers(init?.headers).get("Idempotency-Key")
    );
    expect(keys[0]).toBeTruthy();
    expect(keys[1]).toBe(keys[0]);
  });

  it("requires accepted bindings to exactly match submitted definitions", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const receipts = [
      (() => {
        const receipt = structuredClone(apply.expected.body);
        delete receipt.acceptedDefinitions["tetris.dropInterval"];
        receipt.acceptedDefinitions.extra = {
          definitionId: "def_extra",
          revision: "rev_extra",
          contractDigest: `sha256:${"1".repeat(64)}`,
        };
        return receipt;
      })(),
      (() => {
        const receipt = structuredClone(apply.expected.body);
        receipt.acceptedDefinitions["tetris.dropInterval"]!.contractDigest =
          `sha256:${"2".repeat(64)}`;
        return receipt;
      })(),
      (() => {
        const receipt = structuredClone(apply.expected.body);
        receipt.acceptedDefinitions.extra = {
          definitionId: "def_extra",
          revision: "rev_extra",
          contractDigest: `sha256:${"3".repeat(64)}`,
        };
        return receipt;
      })(),
    ];

    for (const receipt of receipts) {
      const fetch = vi.fn<FetchLike>().mockResolvedValue(
        response(200, receipt),
      );
      await expect(
        createFlaggoClient({
          dataPlaneUrl: "https://data.flaggo.test",
          controlPlane: {
            mode: "startup-register",
            url: "https://control.flaggo.test",
            bundle: apply.request.body,
            credential: { mode: "local-development" },
          },
          appId: "tetris-demo",
          environment: "dev",
          fetch,
        }),
      ).rejects.toBeInstanceOf(InvalidServerResponseError);
    }
  });
});

describe("runtime safety", () => {
  it("rejects same-key definitions that differ from the accepted binding", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>();
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "pre-registered",
        receipt: apply.expected.body,
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });
    const changed = structuredClone(apply.request.body.definitions[0]!);
    changed.actionSpace.default = 850;

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: changed,
        context: {},
      }),
    ).rejects.toBeInstanceOf(ContractConflictError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("returns a provenance-limited local value only for eligible availability failure", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const unavailable = fixture<{
      expected: { status: number; body: unknown };
    }>("errors/fallback-01-service-unavailable-eligible.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(unavailable.expected.status, unavailable.expected.body));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    const result = await client.tune.numberDetailed("tetris.dropInterval", {
      definition: apply.request.body.definitions[0]!,
      context: {},
    });

    expect(result).toEqual(
      expect.objectContaining({
        source: "client-fallback",
        value: 800,
        confidence: null,
      }),
    );
    expect(result).not.toHaveProperty("decisionId");
    expect(result).not.toHaveProperty("policy");
    expect(result).not.toHaveProperty("auditId");
  });

  it("accepts eligible Problem Details when optional prose is omitted", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const unavailable = fixture<{
      expected: { status: number; body: Record<string, unknown> };
    }>("errors/fallback-01-service-unavailable-eligible.json");
    const problem = structuredClone(unavailable.expected.body);
    delete problem.title;
    delete problem.detail;
    delete problem.correlationId;
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(unavailable.expected.status, problem));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({ source: "client-fallback", value: 800 });
  });

  it("rejects malformed structured Problem Details extensions", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(503, {
        type: "https://flaggo.dev/problems/service-unavailable",
        status: 503,
        code: "service-unavailable",
        clientFallback: null,
        issues: "invalid",
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects malformed Problem Details URI references", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(503, {
        type: "not a valid URI reference",
        status: 503,
        code: "service-unavailable",
        clientFallback: { eligible: true },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("surfaces contract errors even when local fallback is configured", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const conflict = fixture<{
      expected: { status: number; body: unknown };
    }>("errors/decide-04-conflicting-contract-identity.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(conflict.expected.status, conflict.expected.body));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(FlaggoHttpError);
  });

  it("uses local fallback for a transport failure only when enabled", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockRejectedValueOnce(networkFailure("ECONNREFUSED"));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({ source: "client-fallback", value: 800 });
  });

  it("retries eligible transport failures with stable retry identity", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockRejectedValueOnce(networkFailure("ENOTFOUND"))
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      ));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default" },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({ source: "server", value: 700 });

    const firstHeaders = new Headers(fetch.mock.calls[1]![1]!.headers);
    const retryHeaders = new Headers(fetch.mock.calls[2]![1]!.headers);
    expect(firstHeaders.get("Idempotency-Key")).toMatch(/^flaggo-sdk:/);
    expect(retryHeaders.get("Idempotency-Key")).toBe(
      firstHeaders.get("Idempotency-Key"),
    );
  });

  it("retries eligible transport failures while reading the response", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const interruptedResponse = {
      status: 200,
      ok: true,
      headers: new Headers(),
      json: vi.fn().mockRejectedValue(
        networkFailure("UND_ERR_BODY_TIMEOUT"),
      ),
    } as unknown as Response;
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(interruptedResponse)
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      ));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default" },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({ source: "server", value: 700 });
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("falls back after retrying intermediary 504 responses", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const gatewayTimeout = () => new Response(null, {
      status: 504,
      headers: { "Retry-After": "0" },
    });
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(gatewayTimeout())
      .mockResolvedValueOnce(gatewayTimeout());
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default" },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({
      source: "client-fallback",
      reason: "intermediary HTTP 504",
    });
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("retries an eligible 503 before using local fallback", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const unavailable = fixture<{
      expected: { status: number; body: unknown };
    }>("errors/fallback-01-service-unavailable-eligible.json");
    const unavailableResponse = () => new Response(
      JSON.stringify(unavailable.expected.body),
      {
        status: unavailable.expected.status,
        headers: {
          "Content-Type": "application/problem+json",
          "Retry-After": "0",
        },
      },
    );
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(unavailableResponse())
      .mockResolvedValueOnce(unavailableResponse());
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default" },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({
      source: "client-fallback",
      value: 800,
    });
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("allows explicitly eligible non-gateway 5xx fallback", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const problem = {
      type: "https://flaggo.dev/problems/insufficient-storage",
      status: 507,
      code: "insufficient-storage",
      clientFallback: {
        eligible: true,
        reason: "temporary-capacity-failure",
      },
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(JSON.stringify(problem), {
        status: 507,
        headers: { "Content-Type": "application/problem+json" },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({
      source: "client-fallback",
      reason: "temporary-capacity-failure",
    });
  });

  it.each([500, 501, 505])(
    "forbids fallback for explicitly excluded HTTP %s",
    async (status) => {
      const apply = fixture<{
        request: { body: DecisionDefinitionBundle };
        expected: { body: RegistrationReceipt };
      }>("management/definition-bundle/04-apply-approved-receipt.json");
      const problem = {
        type: `https://flaggo.dev/problems/http-${status}`,
        status,
        code: `http-${status}`,
        clientFallback: { eligible: true },
      };
      const fetch = vi.fn<FetchLike>()
        .mockResolvedValueOnce(response(200, apply.expected.body))
        .mockResolvedValueOnce(new Response(JSON.stringify(problem), {
          status,
          headers: { "Content-Type": "application/problem+json" },
        }));
      const client = await createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        availabilityFallback: { mode: "local-default", retries: 0 },
        fetch,
      });

      await expect(
        client.tune.number("tetris.dropInterval", {
          definition: apply.request.body.definitions[0]!,
          context: {},
        }),
      ).rejects.toBeInstanceOf(FlaggoHttpError);
    },
  );

  it("does not treat ineligible Flaggo 504 problems as intermediary fallback", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const problem = {
      type: "https://flaggo.dev/problems/gateway-timeout",
      status: 504,
      code: "gateway-timeout",
      clientFallback: { eligible: false },
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(JSON.stringify(problem), {
        status: 504,
        headers: { "Content-Type": "application/problem+json" },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(FlaggoHttpError);
  });

  it("treats generic intermediary Problem Details as gateway fallback", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const genericProblem = {
      type: "https://gateway.example/problems/upstream-timeout",
      title: "Upstream timeout",
      status: 504,
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(JSON.stringify(genericProblem), {
        status: 504,
        headers: { "Content-Type": "application/problem+json" },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).resolves.toMatchObject({
      source: "client-fallback",
      reason: "intermediary HTTP 504",
    });
  });

  it("rejects generic intermediary Problem Details with mismatched status", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const genericProblem = {
      type: "https://gateway.example/problems/upstream-timeout",
      title: "Upstream timeout",
      status: 400,
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(JSON.stringify(genericProblem), {
        status: 504,
        headers: { "Content-Type": "application/problem+json" },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("does not fall back when forbidden responses fail during body reads", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const failures = [
      {
        response: {
          status: 401,
          ok: false,
          headers: new Headers(),
          json: vi.fn().mockRejectedValue(
            networkFailure("UND_ERR_BODY_TIMEOUT"),
          ),
        } as unknown as Response,
        expected: InvalidServerResponseError,
      },
      {
        response: {
          status: 504,
          ok: false,
          headers: new Headers({
            "Content-Type": "application/problem+json",
          }),
          json: vi.fn().mockRejectedValue(
            new DOMException("cancelled", "AbortError"),
          ),
        } as unknown as Response,
        expected: DOMException,
      },
    ];

    for (const item of failures) {
      const fetch = vi.fn<FetchLike>()
        .mockResolvedValueOnce(response(200, apply.expected.body))
        .mockResolvedValueOnce(item.response);
      const client = await createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        availabilityFallback: { mode: "local-default", retries: 0 },
        fetch,
      });

      await expect(
        client.tune.number("tetris.dropInterval", {
          definition: apply.request.body.definitions[0]!,
          context: {},
        }),
      ).rejects.toBeInstanceOf(item.expected);
    }
  });

  it("rejects Problem Details whose status disagrees with HTTP", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const problem = {
      type: "https://flaggo.dev/problems/service-unavailable",
      status: 400,
      code: "service-unavailable",
      clientFallback: { eligible: true },
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(JSON.stringify(problem), {
        status: 503,
        headers: { "Content-Type": "application/problem+json" },
      }));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      availabilityFallback: { mode: "local-default", retries: 0 },
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("does not fall back for abort, TLS, or unknown failures", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const failures = [
      new DOMException("cancelled", "AbortError"),
      networkFailure("CERT_HAS_EXPIRED"),
      networkFailure("ENETUNREACH"),
      new TypeError("unknown fetch failure"),
    ];

    for (const failure of failures) {
      const fetch = vi.fn<FetchLike>()
        .mockResolvedValueOnce(response(200, apply.expected.body))
        .mockRejectedValueOnce(failure);
      const client = await createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        availabilityFallback: { mode: "local-default", retries: 0 },
        fetch,
      });

      await expect(
        client.tune.number("tetris.dropInterval", {
          definition: apply.request.body.definitions[0]!,
          context: {},
        }),
      ).rejects.toBe(failure);
    }
  });

  it("fails closed when retryable outer errors wrap forbidden causes", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const failures = [
      new TypeError("socket failed", {
        cause: {
          code: "UND_ERR_SOCKET",
          cause: new DOMException("cancelled", "AbortError"),
        },
      }),
      new TypeError("socket failed", {
        cause: {
          code: "UND_ERR_SOCKET",
          cause: { code: "CERT_HAS_EXPIRED" },
        },
      }),
      new TypeError("socket failed", {
        cause: {
          code: "UND_ERR_SOCKET",
          cause: { code: "UNABLE_TO_GET_ISSUER_CERT_LOCALLY" },
        },
      }),
      new TypeError("socket failed", {
        cause: {
          code: "UND_ERR_SOCKET",
          cause: { code: "ERR_SSL_WRONG_VERSION_NUMBER" },
        },
      }),
    ];

    for (const failure of failures) {
      const fetch = vi.fn<FetchLike>()
        .mockResolvedValueOnce(response(200, apply.expected.body))
        .mockRejectedValueOnce(failure);
      const client = await createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: {
          mode: "startup-register",
          url: "https://control.flaggo.test",
          bundle: apply.request.body,
          credential: { mode: "local-development" },
        },
        appId: "tetris-demo",
        environment: "dev",
        availabilityFallback: { mode: "local-default", retries: 0 },
        fetch,
      });

      await expect(
        client.tune.number("tetris.dropInterval", {
          definition: apply.request.body.definitions[0]!,
          context: {},
        }),
      ).rejects.toBe(failure);
    }
  });

  it("retries idempotency-in-progress with the same key", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const inProgress = fixture<{
      expected: { status: number; body: unknown };
    }>("errors/decide-09-idempotency-in-progress.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(new Response(
        JSON.stringify(inProgress.expected.body),
        {
          status: inProgress.expected.status,
          headers: {
            "Content-Type": "application/problem+json",
            "Retry-After": "0",
          },
        },
      ))
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      ));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
        idempotencyKey: "idem-key-abc",
      }),
    ).resolves.toMatchObject({ source: "server", value: 700 });
    expect(
      new Headers(fetch.mock.calls[1]![1]!.headers).get("Idempotency-Key"),
    ).toBe("idem-key-abc");
    expect(
      new Headers(fetch.mock.calls[2]![1]!.headers).get("Idempotency-Key"),
    ).toBe("idem-key-abc");
  });

  it("surfaces transport failures when local fallback is disabled", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const failure = new TypeError("network unavailable");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockRejectedValueOnce(failure);
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBe(failure);
  });

  it("supports pre-registered bindings without a control-plane call", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const fetch = vi.fn<FetchLike>().mockResolvedValue(
      response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
          "tetris-web-dev-a",
        ),
      ),
    );
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: { mode: "pre-registered", receipt: apply.expected.body },
      appId: "tetris-demo",
      environment: "dev",
      deploymentId: "tetris-web-dev-a",
      fetch,
    });

    await client.tune.number("tetris.dropInterval", {
      definition: apply.request.body.definitions[0]!,
      context: {},
    });

    expect(fetch).toHaveBeenCalledOnce();
    expect(String(fetch.mock.calls[0]![0])).toContain(":decide");
  });

  it("uses cached definitions with pre-registered bindings", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const fetch = vi.fn<FetchLike>().mockImplementation(async () =>
      response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      )
    );
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "pre-registered",
        receipt: apply.expected.body,
        bundle: apply.request.body,
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await client.tune.number("tetris.dropInterval", { context: {} });
    const runtimeDefinition = structuredClone(
      apply.request.body.definitions[0]!,
    );
    runtimeDefinition.actionSpace.default = 850;
    await client.tune.number("tetris.dropInterval", {
      definition: runtimeDefinition,
      context: {},
    });

    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("requires a local definition when no static bundle is configured", async () => {
    const apply = fixture<{
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>();
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "pre-registered",
        receipt: apply.expected.body,
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", { context: {} }),
    ).rejects.toBeInstanceOf(MissingStaticDefinitionError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("rejects a pre-registered receipt for another application", async () => {
    const apply = fixture<{
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const receipt = structuredClone(apply.expected.body);
    receipt.application = "another-app";

    await expect(
      createFlaggoClient({
        dataPlaneUrl: "https://data.flaggo.test",
        controlPlane: { mode: "pre-registered", receipt },
        appId: "tetris-demo",
        environment: "dev",
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("sends correlation and retry identities only in their dedicated headers", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
          "tetris-web-dev-a",
        ),
      ));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      deploymentId: "tetris-web-dev-a",
      fetch,
    });

    await client.tune.number("tetris.dropInterval", {
      definition: apply.request.body.definitions[0]!,
      context: {},
      correlationId: "trace-123",
      idempotencyKey: "retry-123",
    });

    const init = fetch.mock.calls[1]![1]!;
    const requestHeaders = new Headers(init.headers);
    expect(requestHeaders.get("X-Flaggo-Correlation-Id")).toBe("trace-123");
    expect(requestHeaders.get("Idempotency-Key")).toBe("retry-123");
    const body = JSON.parse(String(init.body)) as Record<string, unknown>;
    expect(body).not.toHaveProperty("correlationId");
    expect(body).not.toHaveProperty("idempotencyKey");
  });

  it("uses bearer providers and canonical input ordering", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const getToken = vi.fn().mockResolvedValue("runtime-token");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(
        200,
        decisionForReceipt(
          decide.expected.body,
          apply.expected.body,
          "tetris.dropInterval",
        ),
      ));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      dataPlaneCredential: { mode: "bearer", getToken },
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await client.tune.number("tetris.dropInterval", {
      definition: apply.request.body.definitions[0]!,
      context: {},
      inputs: [
        { signal: { key: "a" }, value: 1 },
        { signal: { key: "B" }, value: 2 },
      ],
    });

    expect(getToken).toHaveBeenCalledOnce();
    const init = fetch.mock.calls[1]![1]!;
    expect(new Headers(init.headers).get("Authorization")).toBe(
      "Bearer runtime-token",
    );
    const body = JSON.parse(String(init.body)) as {
      inputs: Array<{ signal: { key: string } }>;
    };
    expect(body.inputs.map(({ signal }) => signal.key)).toEqual(["B", "a"]);
  });

  it("rejects a success response with a mismatched accepted tuple", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: Record<string, unknown> } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const invalid = structuredClone(decide.expected.body);
    (invalid.definitionStatus as Record<string, unknown>).revision = "rev_wrong";
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects undeclared runtime definition status identity", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const invalid = decisionForReceipt(
      decide.expected.body,
      apply.expected.body,
      "tetris.dropInterval",
    ) as Record<string, unknown>;
    (invalid.definitionStatus as Record<string, unknown>).artifactDigest =
      "sha256:undeclared-on-runtime-status";
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects an incomplete success response instead of projecting it", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: Record<string, unknown> } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const invalid = decisionForReceipt(
      decide.expected.body,
      apply.expected.body,
      "tetris.dropInterval",
    ) as Record<string, unknown>;
    delete invalid.decisionId;
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects confidence values outside the contract range", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const invalid = decisionForReceipt(
      decide.expected.body,
      apply.expected.body,
      "tetris.dropInterval",
    ) as Record<string, unknown>;
    invalid.confidence = { evidenceQuality: 2 };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("rejects exposure directives with properties from both union branches", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const decide = fixture<{ expected: { body: unknown } }>(
      "runtime/decide/02-active-numeric-strategy.json",
    );
    const invalid = decisionForReceipt(
      decide.expected.body,
      apply.expected.body,
      "tetris.dropInterval",
    ) as Record<string, unknown>;
    invalid.exposure = {
      confirmationRequired: false,
      confirmToken: "must-not-be-present",
    };
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: apply.request.body.definitions[0]!,
        context: {},
      }),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });
});

describe("exposure and telemetry", () => {
  it("confirms exposure only through the explicit confirmation API", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const confirmation = fixture<{
      expected: { body: unknown };
    }>("runtime/exposure-confirmation/01-confirm-success.json");
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, confirmation.expected.body));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await client.exposures.confirm("decision-123", "confirm-abc", {
      appliedAt: "2026-07-29T19:20:00Z",
      correlationId: "game-loop-123",
    });

    expect(fetch.mock.calls[1]![0]).toContain(
      "/v1/exposures/decision-123:confirm",
    );
    const request = fetch.mock.calls[1]![1]!;
    expect(new Headers(request.headers).get("X-Flaggo-Correlation-Id")).toBe(
      "game-loop-123",
    );
    expect(JSON.parse(String(request.body))).not.toHaveProperty("correlationId");
  });

  it("rejects schema-invalid exposure confirmation results", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const confirmation = fixture<{
      expected: { body: Record<string, unknown> };
    }>("runtime/exposure-confirmation/01-confirm-success.json");
    const invalid = structuredClone(confirmation.expected.body);
    invalid.confirmedAt = "2026-07-29T19:20:00+01:00";
    invalid.unexpected = true;
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const client = await createFlaggoClient({
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register",
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    });

    await expect(
      client.exposures.confirm("decision-123", "confirm-abc"),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("matches contract RFC 3339 UTC timestamp semantics", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const confirmation = fixture<{
      expected: { body: Record<string, unknown> };
    }>("runtime/exposure-confirmation/01-confirm-success.json");
    const valid = structuredClone(confirmation.expected.body);
    valid.confirmedAt = "2026-07-29t19:20:00Z";
    const invalid = structuredClone(confirmation.expected.body);
    invalid.confirmedAt = "2026-07-29T19:20:60Z";
    const fetch = vi.fn<FetchLike>()
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, valid))
      .mockResolvedValueOnce(response(200, apply.expected.body))
      .mockResolvedValueOnce(response(200, invalid));
    const config = {
      dataPlaneUrl: "https://data.flaggo.test",
      controlPlane: {
        mode: "startup-register" as const,
        url: "https://control.flaggo.test",
        bundle: apply.request.body,
        credential: { mode: "local-development" as const },
      },
      appId: "tetris-demo",
      environment: "dev",
      fetch,
    };
    const validClient = await createFlaggoClient(config);
    await expect(
      validClient.exposures.confirm("decision-123", "confirm-abc"),
    ).resolves.toMatchObject({ status: "confirmed" });

    const invalidClient = await createFlaggoClient(config);
    await expect(
      invalidClient.exposures.confirm("decision-123", "confirm-abc"),
    ).rejects.toBeInstanceOf(InvalidServerResponseError);
  });

  it("creates typed inference inputs and emits through the configured sink", () => {
    const emit = vi.fn();
    const signal = createInferenceSignalHandle(
      {
        kind: "metric",
        key: "tetris.boardPressure",
        type: "number",
        source: "app-emitted",
      },
      { emit },
    );

    expect(signal.input(0.82)).toEqual({
      signal: { key: "tetris.boardPressure" },
      value: 0.82,
    });
    signal.emit(0.82);
    expect(emit).toHaveBeenCalledWith(
      expect.objectContaining({
        signal: expect.objectContaining({ key: "tetris.boardPressure" }),
        value: 0.82,
      }),
    );
  });

  it("projects telemetry through an OpenTelemetry-compatible logger", () => {
    const emit = vi.fn();
    const sink = createOpenTelemetrySink({ emit });
    const signal = createSignalHandle(
      {
        kind: "event",
        key: "tetris.piecePlaced",
        fields: { hardDrop: "boolean" },
      },
      sink,
    );

    signal.emit({ hardDrop: true });

    expect(emit).toHaveBeenCalledWith(
      expect.objectContaining({
        body: "flaggo.signal",
        attributes: expect.objectContaining({
          "flaggo.signal.key": "tetris.piecePlaced",
          "flaggo.signal.value": "{\"hardDrop\":true}",
        }),
        timestamp: expect.any(Date),
      }),
    );
  });

  it("creates non-emitting handles for derived metrics", () => {
    const signal = createDerivedMetricHandle({
      kind: "metric",
      key: "tetris.earlyLossRate24h",
      type: "number",
      source: "derived",
      from: [{ key: "tetris.sessionEnded" }],
      aggregation: "rate(endReason == 'early_loss')",
      window: "24h",
    });

    expect(signal).toMatchObject({
      key: "tetris.earlyLossRate24h",
      valueType: "number",
    });
    expect(signal).not.toHaveProperty("emit");
  });
});
