import {
  FlaggoError,
  FlaggoHttpError,
  InvalidDecisionInputError,
  InvalidServerResponseError,
  MissingAcceptedDefinitionError,
} from "./errors.js";
import { SDK_VERSION } from "./package-version.js";
import { assertCatalog, inputMatches, pointer, stepAligned } from "./static-schema.js";
import {
  authorization, headers, parseJson, problemOrThrow, record, hasOwn, isProblem,
  isVerifiedNumberResult, isExposureConfirmationResult, verifyReceiptBindings,
  type CredentialProvider,
} from "./wire.js";
import type {
  AcceptedDefinition,
  CatalogDecision,
  ClientFallbackResult,
  DecisionReceipt,
  DecisionResult,
  ExposureConfirmationResult,
  FetchLike,
  NumberTuneRequest,
  RegistrationReceipt,
  RuntimeCatalog,
  RuntimeContractIdentity,
} from "./types.js";

export type { CredentialProvider } from "./wire.js";

export interface FlaggoClientConfig<C extends RuntimeCatalog = RuntimeCatalog> {
  dataPlaneUrl: string;
  catalog: C;
  receipt: RegistrationReceipt;
  deploymentId?: string;
  dataPlaneCredential?: CredentialProvider;
  availabilityFallback?: {
    mode: "disabled" | "local-default";
    retries?: 0 | 1 | 2;
  };
  fetch?: FetchLike;
}

type NumberKey<C extends RuntimeCatalog> = string extends keyof C["decisions"] ? string : {
  [K in keyof C["decisions"]]: C["decisions"][K]["result"] extends { type: "number" } ? K : never;
}[keyof C["decisions"]] & string;

type Primitive<T> = T extends { type: "number" } ? number : T extends { type: "boolean" } ? boolean : string;
type CallerKeys<D extends CatalogDecision> = {
  [K in keyof D["inputs"]]: D["inputs"][K] extends { source: "request" } ? K : never;
}[keyof D["inputs"]];
type RequiredContextKeys<D extends CatalogDecision> = {
  [K in keyof D["context"]]: D["context"][K] extends { required: true } ? K : never;
}[keyof D["context"]];
type EmptyOr<T> = keyof T extends never ? Record<string, never> : T;
type Context<D extends CatalogDecision> = EmptyOr<
  { [K in RequiredContextKeys<D>]: Primitive<D["context"][K]> }
  & { [K in Exclude<keyof D["context"], RequiredContextKeys<D>>]?: Primitive<D["context"][K]> }
>;
type Inputs<D extends CatalogDecision> = EmptyOr<{ [K in CallerKeys<D>]: Primitive<D["inputs"][K]> }>;
type Request<D extends CatalogDecision> = Omit<NumberTuneRequest, "context" | "inputs">
  & (RequiredContextKeys<D> extends never ? { context?: Context<D> } : { context: Context<D> })
  & (CallerKeys<D> extends never ? { inputs?: Inputs<D> } : { inputs: Inputs<D> });
type CallArguments<C extends RuntimeCatalog, K extends keyof C["decisions"]> =
  string extends keyof C["decisions"] ? [decisionKey: K, request?: NumberTuneRequest]
    : K extends keyof C["decisions"]
      ? RequiredContextKeys<C["decisions"][K]> | CallerKeys<C["decisions"][K]> extends never
        ? [decisionKey: K, request?: Request<C["decisions"][K]>]
        : [decisionKey: K, request: Request<C["decisions"][K]>]
      : never;

export interface FlaggoClient<C extends RuntimeCatalog = RuntimeCatalog> {
  readonly tune: {
    number<K extends NumberKey<C>>(
      ...args: CallArguments<C, K>
    ): Promise<DecisionReceipt<number>>;
    numberDetailed<K extends NumberKey<C>>(
      ...args: CallArguments<C, K>
    ): Promise<DecisionResult<number>>;
  };
  readonly exposures: {
    confirm(
      decisionId: string,
      confirmToken: string,
      options?: { appliedAt?: string; correlationId?: string },
    ): Promise<ExposureConfirmationResult>;
  };
}

function expectedIdentity(
  receipt: RegistrationReceipt,
  decisionKey: string,
  accepted: AcceptedDefinition,
  config: FlaggoClientConfig,
): RuntimeContractIdentity {
  return {
    ...accepted,
    bundleDigest: receipt.bundleDigest,
    ...(receipt.buildId === undefined ? {} : { buildId: receipt.buildId }),
    ...(receipt.artifactDigest === undefined
      ? {}
      : { artifactDigest: receipt.artifactDigest }),
    ...(config.deploymentId === undefined
      ? {}
      : { deploymentId: config.deploymentId }),
  };
}

function projectReceipt(
  result: DecisionResult<number>,
): DecisionReceipt<number> {
  if (result.source === "client-fallback") {
    return {
      source: result.source,
      value: result.value,
      expectedContract: result.expectedContract,
      reason: result.reason,
    };
  }
  return {
    source: result.source,
    value: result.value,
    decisionId: result.decisionId,
    exposure: result.exposure,
  };
}

function validateRequest(definition: CatalogDecision, request: NumberTuneRequest): void {
  const value = record(request);
  const allowed = new Set(["context", "inputs", "runtimeTarget", "idempotencyKey", "correlationId"]);
  if (value === undefined || Object.keys(value).some((key) => !allowed.has(key))) {
    throw new InvalidDecisionInputError("/", "A request may contain only runtime data and request metadata.");
  }
  const context = record(request.context ?? {});
  const inputs = record(request.inputs ?? {});
  if (request.context === null || context === undefined) {
    throw new InvalidDecisionInputError("/runtimeContext", "Context must be an object.", "invalid-runtime-context");
  }
  if (request.inputs === null || inputs === undefined) {
    throw new InvalidDecisionInputError("/inputs", "Inputs must be a plain object, not signal references.");
  }
  for (const [key, field] of Object.entries(definition.context)) {
    if (!Object.hasOwn(context, key)) {
      if (field.required) throw new InvalidDecisionInputError(`/runtimeContext/${pointer(key)}`, "Required context is missing.", "invalid-runtime-context");
    } else if (!inputMatches(context[key], field)
      || (field.target !== undefined && (typeof context[key] !== "string" || context[key].trim().length === 0))) {
      throw new InvalidDecisionInputError(`/runtimeContext/${pointer(key)}`, "Context does not satisfy its declared type or target.", "invalid-runtime-context");
    }
  }
  for (const key of Object.keys(context)) {
    if (!Object.hasOwn(definition.context, key)) {
      throw new InvalidDecisionInputError(`/runtimeContext/${pointer(key)}`, "Unknown context field.", "invalid-runtime-context");
    }
  }
  for (const [key, schema] of Object.entries(definition.inputs)) {
    const present = Object.hasOwn(inputs, key);
    if (schema.source === "evidence") {
      if (present) throw new InvalidDecisionInputError(`/inputs/${pointer(key)}`, "Evidence-owned inputs cannot be supplied by callers.", "input-source-conflict");
    } else if (!present || !inputMatches(inputs[key], schema)) {
      throw new InvalidDecisionInputError(`/inputs/${pointer(key)}`, "Required input is missing or violates its primitive type/bounds.");
    }
  }
  for (const key of Object.keys(inputs)) {
    if (!Object.hasOwn(definition.inputs, key)) {
      throw new InvalidDecisionInputError(`/inputs/${pointer(key)}`, "Unknown input.");
    }
  }
  if (request.runtimeTarget !== undefined) {
    const target = record(request.runtimeTarget);
    if (target === undefined || Object.keys(target).some((key) => key !== "type" && key !== "id")
      || typeof target.type !== "string" || target.type.trim().length === 0
      || typeof target.id !== "string" || target.id.trim().length === 0) {
      throw new InvalidDecisionInputError("/runtimeTarget", "A target requires a nonempty type and ID.", "invalid-runtime-target");
    }
  }
  for (const key of ["idempotencyKey", "correlationId"] as const) {
    if (request[key] !== undefined && (typeof request[key] !== "string" || request[key].length === 0)) {
      throw new InvalidDecisionInputError(`/${key}`, "Request metadata must be a nonempty string.");
    }
  }
}

const RETRYABLE_TRANSPORT_CODES = new Set([
  "EAI_AGAIN",
  "ECONNREFUSED",
  "ECONNRESET",
  "ENOTFOUND",
  "ETIMEDOUT",
  "UND_ERR_BODY_TIMEOUT",
  "UND_ERR_CONNECT_TIMEOUT",
  "UND_ERR_HEADERS_TIMEOUT",
  "UND_ERR_SOCKET",
]);

function isRetryableTransportFailure(error: unknown): boolean {
  const chain: Array<{
    name?: unknown;
    code?: unknown;
  }> = [];
  let current = error;
  const visited = new Set<unknown>();
  while (
    current !== null
    && typeof current === "object"
    && !visited.has(current)
  ) {
    visited.add(current);
    const candidate = current as {
      name?: unknown;
      code?: unknown;
      cause?: unknown;
    };
    chain.push(candidate);
    current = candidate.cause;
  }
  if (
    chain.some(({ name }) => name === "AbortError")
    || chain.some(
      ({ code }) =>
        typeof code === "string"
        && !RETRYABLE_TRANSPORT_CODES.has(code),
    )
  ) return false;
  return chain.some(({ name, code }) =>
    name === "TimeoutError"
    || (
      typeof code === "string"
      && RETRYABLE_TRANSPORT_CODES.has(code)
    )
  );
}

function errorChainHasName(error: unknown, name: string): boolean {
  return findErrorInChain(error, name) !== undefined;
}

function findErrorInChain(error: unknown, name: string): unknown {
  let current = error;
  const visited = new Set<unknown>();
  while (
    current !== null
    && typeof current === "object"
    && !visited.has(current)
  ) {
    visited.add(current);
    const candidate = current as { name?: unknown; cause?: unknown };
    if (candidate.name === name) return current;
    current = candidate.cause;
  }
  return undefined;
}

function retryDelayMilliseconds(response?: Response): number {
  const retryAfter = response?.headers.get("Retry-After");
  if (retryAfter !== null && retryAfter !== undefined) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds) && seconds >= 0) {
      return Math.min(seconds * 1_000, 1_000);
    }
    const date = Date.parse(retryAfter);
    if (Number.isFinite(date)) {
      return Math.min(Math.max(date - Date.now(), 0), 1_000);
    }
  }
  return 50 + Math.floor(Math.random() * 101);
}

function permitsEligibleFlaggoFallback(status: number): boolean {
  return status >= 500
    && status <= 599
    && status !== 500
    && status !== 501
    && status !== 505;
}

async function waitBeforeRetry(response?: Response): Promise<void> {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, retryDelayMilliseconds(response));
  });
}

export function createFlaggoClient<const C extends RuntimeCatalog>(
  configuration: FlaggoClientConfig<C>,
): FlaggoClient<C> {
  const allowed = new Set([
    "dataPlaneUrl", "catalog", "receipt", "deploymentId", "dataPlaneCredential",
    "availabilityFallback", "fetch",
  ]);
  if (Object.keys(configuration).some((key) => !allowed.has(key))) {
    throw new FlaggoError("Unsupported runtime client configuration field.");
  }
  assertCatalog(configuration.catalog);
  const catalog = structuredClone(configuration.catalog);
  const receipt = structuredClone(configuration.receipt);
  verifyReceiptBindings(receipt, catalog.application, catalog.bundleDigest, catalog.decisions);
  const config = {
    ...configuration,
    catalog,
    receipt,
    ...(configuration.availabilityFallback === undefined ? {} : {
      availabilityFallback: { ...configuration.availabilityFallback },
    }),
  };
  if (config.availabilityFallback !== undefined
    && (!["disabled", "local-default"].includes(config.availabilityFallback.mode)
      || ![0, 1, 2].includes(config.availabilityFallback.retries ?? 1))) {
    throw new FlaggoError("Invalid availability fallback configuration.");
  }
  const fetch = config.fetch ?? globalThis.fetch.bind(globalThis);
  const definitions = new Map(Object.entries(catalog.decisions));

  async function numberDetailed(
    decisionKey: string,
    request: NumberTuneRequest = {},
  ): Promise<DecisionResult<number>> {
    request = structuredClone(request);
    const definition = definitions.get(decisionKey);
    if (definition === undefined) {
      throw new MissingAcceptedDefinitionError(decisionKey);
    }
    if (definition.result.type !== "number") {
      throw new FlaggoError(`Decision '${decisionKey}' does not return a number.`);
    }
    validateRequest(definition, request);
    const accepted = receipt.acceptedDefinitions[decisionKey]!;
    const expectedContract = expectedIdentity(
      receipt,
      decisionKey,
      accepted,
      config,
    );
    const auth = await authorization(config.dataPlaneCredential);
    const fallbackEnabled =
      config.availabilityFallback?.mode === "local-default";
    const retries = config.availabilityFallback?.retries ?? 1;
    const idempotencyKey = request.idempotencyKey
      ?? (
        fallbackEnabled && retries > 0
          ? `flaggo-sdk:${globalThis.crypto.randomUUID()}`
          : undefined
      );
    const requestBody = JSON.stringify({
      expectedContract,
      ...(request.runtimeTarget === undefined
        ? {}
        : { runtimeTarget: request.runtimeTarget }),
      runtimeContext: request.context ?? {},
      ...(request.inputs === undefined
        ? {}
        : {
            inputs: request.inputs,
          }),
      client: {
        appId: catalog.application.id,
        environment: catalog.application.environment,
        sdk: "typescript",
        sdkVersion: SDK_VERSION,
      },
    });
    const requestInit: RequestInit = {
      method: "POST",
      headers: {
        ...Object.fromEntries(headers(auth, request.correlationId)),
        ...(idempotencyKey === undefined
          ? {}
          : { "Idempotency-Key": idempotencyKey }),
      },
      body: requestBody,
    };
    const url =
      `${config.dataPlaneUrl.replace(/\/$/, "")}/v1/decisions/${encodeURIComponent(decisionKey)}:decide`;

    for (let attempt = 0; attempt <= retries; attempt += 1) {
      let response: Response;
      try {
        response = await fetch(url, requestInit);
      } catch (error) {
        if (!fallbackEnabled || !isRetryableTransportFailure(error)) throw error;
        if (attempt < retries) {
          await waitBeforeRetry();
          continue;
        }
        return localFallback(
          decisionKey,
          definition.result.default,
          expectedContract,
          "data-plane transport failure",
        );
      }

      const intermediaryStatus =
        response.status === 502 || response.status === 504;
      const problemContentType = response.headers
        .get("Content-Type")
        ?.toLowerCase()
        .includes("application/problem+json") === true;
      let body: unknown;
      let bodyParsed = false;
      if (intermediaryStatus && problemContentType) {
        try {
          body = await parseJson(response);
          bodyParsed = true;
        } catch (error) {
          const genericIntermediaryBody =
            errorChainHasName(error, "SyntaxError");
          const cancellation = findErrorInChain(error, "AbortError");
          if (cancellation !== undefined) throw cancellation;
          if (
            !fallbackEnabled
            || (
              !isRetryableTransportFailure(error)
              && !genericIntermediaryBody
            )
          ) {
            throw error;
          }
          if (attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          return localFallback(
            decisionKey,
            definition.result.default,
            expectedContract,
            `intermediary HTTP ${response.status}`,
          );
        }
        const genericProblem = record(body);
        if (
          genericProblem !== undefined
          && hasOwn(genericProblem, "status")
          && (
            !Number.isInteger(genericProblem.status)
            || genericProblem.status !== response.status
          )
        ) {
          throw new InvalidServerResponseError(
            `Problem Details status does not match HTTP ${response.status}.`,
          );
        }
      }
      if (
        intermediaryStatus
        && (!problemContentType || !isProblem(body))
      ) {
        if (!fallbackEnabled) {
          if (!bodyParsed) body = await parseJson(response);
          throw new InvalidServerResponseError(
            `Intermediary HTTP ${response.status} is not Flaggo Problem Details.`,
          );
        }
        if (attempt < retries) {
          await waitBeforeRetry(response);
          continue;
        }
        return localFallback(
          decisionKey,
          definition.result.default,
          expectedContract,
          `intermediary HTTP ${response.status}`,
        );
      }

      if (!bodyParsed) {
        try {
          body = await parseJson(response);
        } catch (error) {
          const cancellation = findErrorInChain(error, "AbortError");
          if (cancellation !== undefined) throw cancellation;
          if (
            !fallbackEnabled
            || !response.ok
            || !isRetryableTransportFailure(error)
          ) {
            throw error;
          }
          if (attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          return localFallback(
            decisionKey,
            definition.result.default,
            expectedContract,
            "data-plane response transport failure",
          );
        }
      }
      if (!response.ok) {
        const problem = problemOrThrow(body, response.status);
        if (
          response.status === 409
          && problem.code === "idempotency-in-progress"
          && idempotencyKey !== undefined
          && attempt < retries
        ) {
          await waitBeforeRetry(response);
          continue;
        }
        if (
          permitsEligibleFlaggoFallback(response.status)
          && problem.clientFallback?.eligible === true
        ) {
          if (fallbackEnabled && attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          if (fallbackEnabled) {
            return localFallback(
              decisionKey,
              definition.result.default,
              expectedContract,
              problem.clientFallback.reason ?? problem.code,
            );
          }
        }
        throw new FlaggoHttpError(problem);
      }
      if (
        !isVerifiedNumberResult(
          body,
          decisionKey,
          expectedContract,
          catalog.application.id,
          catalog.application.environment,
        )
        || body.value < definition.result.min || body.value > definition.result.max
        || (definition.result.step !== undefined
          && !stepAligned(body.value, definition.result.min, definition.result.step))
      ) {
        throw new InvalidServerResponseError(
          `Decision '${decisionKey}' returned an invalid or unverified result.`,
        );
      }
      return { ...body, source: "server" };
    }
    throw new Error("Unreachable retry state.");
  }

  return {
    tune: {
      async number(decisionKey: string, request?: NumberTuneRequest) {
        return projectReceipt(await numberDetailed(decisionKey, request));
      },
      numberDetailed,
    },
    exposures: {
      async confirm(decisionId, confirmToken, options = {}) {
        const auth = await authorization(config.dataPlaneCredential);
        const response = await fetch(
          `${config.dataPlaneUrl.replace(/\/$/, "")}/v1/exposures/${encodeURIComponent(decisionId)}:confirm`,
          {
            method: "POST",
            headers: headers(auth, options.correlationId),
            body: JSON.stringify({
              confirmToken,
              ...(options.appliedAt === undefined
                ? {}
                : { appliedAt: options.appliedAt }),
            }),
          },
        );
        const body = await parseJson(response);
        if (!response.ok) {
          throw new FlaggoHttpError(problemOrThrow(body, response.status));
        }
        if (!isExposureConfirmationResult(body, decisionId)) {
          throw new InvalidServerResponseError(
            "Flaggo returned a malformed exposure confirmation.",
          );
        }
        return body;
      },
    },
  };
}

function localFallback(
  decisionKey: string,
  value: number,
  expectedContract: RuntimeContractIdentity,
  reason: string,
): ClientFallbackResult<number> {
  return {
    source: "client-fallback",
    decisionKey,
    expectedContract,
    value,
    valueType: "number",
    decisionMode: "fallback",
    confidence: null,
    reason,
    fallback: {
      source: "client-fallback",
      resolutionFallbackUsed: false,
      decisionFallbackUsed: true,
      reason,
    },
  };
}

export function confirmedExposureAttributes(
  confirmation: ExposureConfirmationResult,
): Readonly<{ "flaggo.exposure.id": string; "flaggo.decision.id": string }> {
  const value = record(confirmation);
  if (value === undefined || typeof value.decisionId !== "string" || value.decisionId.length === 0
    || !isExposureConfirmationResult(value, value.decisionId)) {
    throw new FlaggoError("A completed explicit exposure confirmation is required.");
  }
  return Object.freeze({
    "flaggo.exposure.id": value.exposureId,
    "flaggo.decision.id": value.decisionId,
  });
}
