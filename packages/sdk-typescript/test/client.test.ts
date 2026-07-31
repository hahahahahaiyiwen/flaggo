import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";

import {
  ContractConflictError,
  FlaggoHttpError,
  InvalidServerResponseError,
  RequiresApprovalError,
  contractDigest,
  createDerivedMetricHandle,
  createInferenceSignalHandle,
  createFlaggoClient,
  createOpenTelemetrySink,
  createSignalHandle,
  normalizeBundle,
  signalSchemaDigest,
  type DecisionDefinitionBundle,
  type FetchLike,
  type RegistrationReceipt,
} from "../src/index.js";

const repositoryRoot = resolve(import.meta.dirname, "../../..");

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
    ...(receipt.artifactDigest === undefined
      ? {}
      : { artifactDigest: receipt.artifactDigest }),
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
      createSignalHandle<number>({ ...declaration, schemaDigest: digest }, {
        emit() {},
      }).schemaDigest,
    ).toBe(digest);
    expect(() =>
      createSignalHandle<number>({
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
      definition: apply.request.body.definitions[0]!,
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
});

describe("runtime safety", () => {
  it("rejects same-key definitions that differ from the accepted binding", async () => {
    const apply = fixture<{
      request: { body: DecisionDefinitionBundle };
      expected: { body: RegistrationReceipt };
    }>("management/definition-bundle/04-apply-approved-receipt.json");
    const fetch = vi.fn<FetchLike>().mockResolvedValue(
      response(200, apply.expected.body),
    );
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
    const changed = structuredClone(apply.request.body.definitions[0]!);
    changed.actionSpace.default = 850;

    await expect(
      client.tune.number("tetris.dropInterval", {
        definition: changed,
        context: {},
      }),
    ).rejects.toBeInstanceOf(ContractConflictError);
    expect(fetch).toHaveBeenCalledTimes(1);
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
      availabilityFallback: { mode: "local-default" },
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
      availabilityFallback: { mode: "local-default" },
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
      .mockRejectedValueOnce(new TypeError("network unavailable"));
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
    ).resolves.toMatchObject({ source: "client-fallback", value: 800 });
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

  it("creates typed inference inputs and emits through the configured sink", () => {
    const emit = vi.fn();
    const signal = createInferenceSignalHandle<number>(
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
    const signal = createSignalHandle<{ hardDrop: boolean }>(
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
    const signal = createDerivedMetricHandle<number>({
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
