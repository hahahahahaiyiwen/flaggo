import {
  FlaggoHttpError,
  InvalidDecisionInputError,
  InvalidServerResponseError,
  MissingContractBindingError,
} from "./errors.js";
import {
  authorization,
  hasOnlyKeys,
  headers,
  isDecisionName,
  isSha256Digest,
  parseJson,
  problemOrThrow,
  record,
  responseMetadata,
  type CredentialProvider,
} from "./wire.js";
import type {
  FetchLike,
  JsonValue,
  RuntimeContractBindings,
  RuntimeDecision,
  RuntimeDecisionRequest,
  RuntimeInput,
  Sha256Digest,
} from "./types.js";

const attributeNamePattern = /^[A-Za-z][A-Za-z0-9_]{0,127}$/u;
const ruleNamePattern = /^[A-Za-z][A-Za-z0-9._-]{0,127}$/u;
const maximumAttributes = 128;
const maximumDepth = 16;
const maximumCollectionSize = 256;
const maximumStringBytes = 16_384;
const maximumRetryDelayMilliseconds = 1_000;

export interface FlaggoClientConfig<
  C extends RuntimeContractBindings = RuntimeContractBindings,
> {
  decisionServiceUrl: string;
  contracts: C;
  credential?: CredentialProvider;
  retries?: number;
  fetch?: FetchLike;
  random?: () => number;
}

type ContractName<C extends RuntimeContractBindings> = Extract<keyof C, string>;

export interface FlaggoClient<
  C extends RuntimeContractBindings = RuntimeContractBindings,
> {
  decide<TResult extends JsonValue = JsonValue>(
    contractName: ContractName<C>,
    request?: RuntimeDecisionRequest,
  ): Promise<RuntimeDecision<TResult>>;
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function validateJsonValue(value: unknown, path: string, depth = 0): asserts value is JsonValue {
  if (depth > maximumDepth) {
    throw new InvalidDecisionInputError(path, `Values may be nested at most ${maximumDepth} levels.`);
  }
  if (value === null || typeof value === "boolean") return;
  if (typeof value === "string") {
    if (byteLength(value) > maximumStringBytes) {
      throw new InvalidDecisionInputError(
        path,
        `Strings may contain at most ${maximumStringBytes} UTF-8 bytes.`,
      );
    }
    return;
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new InvalidDecisionInputError(path, "Numbers must be finite.");
    }
    return;
  }
  if (Array.isArray(value)) {
    if (value.length > maximumCollectionSize) {
      throw new InvalidDecisionInputError(
        path,
        `Arrays may contain at most ${maximumCollectionSize} items.`,
      );
    }
    value.forEach((item, index) => validateJsonValue(item, `${path}/${index}`, depth + 1));
    return;
  }
  const object = record(value);
  if (object === undefined) {
    throw new InvalidDecisionInputError(path, "Values must be JSON values.");
  }
  if (Object.keys(object).length > maximumCollectionSize) {
    throw new InvalidDecisionInputError(
      path,
      `Objects may contain at most ${maximumCollectionSize} properties.`,
    );
  }
  for (const [key, item] of Object.entries(object)) {
    if (key.length === 0 || byteLength(key) > maximumStringBytes) {
      throw new InvalidDecisionInputError(
        path,
        `Object property names must contain 1-${maximumStringBytes} UTF-8 bytes.`,
      );
    }
    validateJsonValue(item, `${path}/${key}`, depth + 1);
  }
}

function cloneRequest(request: RuntimeDecisionRequest, random: () => number): {
  input: RuntimeInput;
  correlationId?: string;
} {
  const raw = record(request);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["attributes", "currentExposure", "correlationId"]))
  ) {
    throw new InvalidDecisionInputError("/", "The decision request contains unknown members.");
  }

  const rawAttributes = request.attributes ?? {};
  const attributes = record(rawAttributes);
  if (attributes === undefined) {
    throw new InvalidDecisionInputError("/attributes", "Attributes must be an object.");
  }
  if (Object.keys(attributes).length > maximumAttributes) {
    throw new InvalidDecisionInputError(
      "/attributes",
      `At most ${maximumAttributes} application attributes may be supplied.`,
    );
  }

  const completeAttributes: Record<string, JsonValue> = {};
  for (const [name, value] of Object.entries(attributes)) {
    if (!attributeNamePattern.test(name)) {
      throw new InvalidDecisionInputError(
        `/attributes/${name}`,
        "Attribute names must begin with a letter and contain only letters, digits, and underscores; '_' is reserved for the Flaggo SDK.",
      );
    }
    validateJsonValue(value, `/attributes/${name}`);
    completeAttributes[name] = structuredClone(value);
  }

  const randomValue = random();
  if (!Number.isFinite(randomValue) || randomValue < 0 || randomValue >= 1) {
    throw new InvalidDecisionInputError(
      "/attributes/_random",
      "The SDK random source must return a finite number in [0, 1).",
    );
  }
  completeAttributes._random = randomValue;

  let currentExposure: RuntimeInput["currentExposure"];
  if (request.currentExposure !== undefined) {
    const exposure = record(request.currentExposure);
    if (
      exposure === undefined
      || !hasOnlyKeys(exposure, new Set(["exposureId"]))
      || typeof request.currentExposure.exposureId !== "string"
      || request.currentExposure.exposureId.length === 0
      || byteLength(request.currentExposure.exposureId) > 256
    ) {
      throw new InvalidDecisionInputError(
        "/currentExposure/exposureId",
        "Exposure IDs must contain 1-256 UTF-8 bytes.",
      );
    }
    currentExposure = { exposureId: request.currentExposure.exposureId };
  }

  if (
    request.correlationId !== undefined
    && (
      request.correlationId.length === 0
      || byteLength(request.correlationId) > 256
      || /[\r\n]/u.test(request.correlationId)
    )
  ) {
    throw new InvalidDecisionInputError(
      "/correlationId",
      "Correlation IDs must contain 1-256 UTF-8 bytes without line breaks.",
    );
  }

  return {
    input: {
      attributes: completeAttributes,
      ...(currentExposure === undefined ? {} : { currentExposure }),
    },
    ...(request.correlationId === undefined
      ? {}
      : { correlationId: request.correlationId }),
  };
}

function validateConfig(
  configuration: FlaggoClientConfig,
): {
  decisionServiceUrl: string;
  contracts: RuntimeContractBindings;
  credential?: CredentialProvider;
  retries: number;
  fetch: FetchLike;
  random: () => number;
} {
  const value = record(configuration);
  const allowed = new Set([
    "decisionServiceUrl",
    "contracts",
    "credential",
    "retries",
    "fetch",
    "random",
  ]);
  if (value === undefined || !hasOnlyKeys(value, allowed)) {
    throw new InvalidDecisionInputError("/", "The client configuration contains unknown members.");
  }
  if (
    typeof configuration.decisionServiceUrl !== "string"
    || !URL.canParse(configuration.decisionServiceUrl)
  ) {
    throw new InvalidDecisionInputError(
      "/decisionServiceUrl",
      "Decision Service URL must be an absolute URL.",
    );
  }
  const contracts = record(configuration.contracts);
  if (contracts === undefined || Object.keys(contracts).length === 0) {
    throw new InvalidDecisionInputError(
      "/contracts",
      "At least one runtime contract binding is required.",
    );
  }
  for (const [name, rawBinding] of Object.entries(contracts)) {
    const binding = record(rawBinding);
    if (
      !isDecisionName(name)
      || binding === undefined
      || !hasOnlyKeys(binding, new Set(["contractDigest"]))
      || !isSha256Digest(binding.contractDigest)
    ) {
      throw new InvalidDecisionInputError(
        `/contracts/${name}`,
        "Each binding requires a valid contract name and contractDigest.",
      );
    }
  }
  const retries = configuration.retries ?? 0;
  if (!Number.isInteger(retries) || retries < 0 || retries > 2) {
    throw new InvalidDecisionInputError("/retries", "Retries must be an integer from 0 to 2.");
  }
  if (configuration.fetch !== undefined && typeof configuration.fetch !== "function") {
    throw new InvalidDecisionInputError("/fetch", "Fetch must be a function.");
  }
  if (configuration.random !== undefined && typeof configuration.random !== "function") {
    throw new InvalidDecisionInputError("/random", "Random must be a function.");
  }
  return {
    decisionServiceUrl: configuration.decisionServiceUrl.replace(/\/+$/u, ""),
    contracts: structuredClone(configuration.contracts),
    ...(configuration.credential === undefined
      ? {}
      : { credential: configuration.credential }),
    retries,
    fetch: configuration.fetch ?? globalThis.fetch.bind(globalThis),
    random: configuration.random ?? Math.random,
  };
}

function decisionOrThrow<TResult extends JsonValue>(
  value: unknown,
  contractDigest: Sha256Digest,
): RuntimeDecision<TResult> {
  const decision = record(value);
  const evaluation = record(decision?.evaluation);
  if (
    decision === undefined
    || !hasOnlyKeys(
      decision,
      new Set(["contractDigest", "executableDigest", "result", "evaluation"]),
    )
    || decision.contractDigest !== contractDigest
    || !isSha256Digest(decision.executableDigest)
    || evaluation === undefined
    || !hasOnlyKeys(evaluation, new Set(["source", "rule"]))
    || (
      evaluation.source !== "default"
      && evaluation.source !== "rule"
    )
    || (
      evaluation.source === "default"
      && evaluation.rule !== undefined
    )
    || (
      evaluation.source === "rule"
      && (typeof evaluation.rule !== "string" || !ruleNamePattern.test(evaluation.rule))
    )
  ) {
    throw new InvalidServerResponseError("Flaggo returned a malformed RuntimeDecision.");
  }
  try {
    validateJsonValue(decision.result, "/result");
  } catch (error) {
    throw new InvalidServerResponseError(
      "Flaggo returned an invalid RuntimeDecision result.",
      { cause: error },
    );
  }
  return structuredClone(value) as RuntimeDecision<TResult>;
}

function isRetryableStatus(status: number): boolean {
  return status === 429 || status === 502 || status === 503 || status === 504;
}

async function waitBeforeRetry(response?: Response): Promise<void> {
  const retryAfter = response?.headers.get("Retry-After");
  let milliseconds = 0;
  if (retryAfter !== null && retryAfter !== undefined) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds) && seconds >= 0) {
      milliseconds = Math.min(
        maximumRetryDelayMilliseconds,
        Math.round(seconds * 1_000),
      );
    }
  }
  if (milliseconds > 0) {
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
  }
}

export function createFlaggoClient<const C extends RuntimeContractBindings>(
  configuration: FlaggoClientConfig<C>,
): FlaggoClient<C> {
  const config = validateConfig(configuration);

  return {
    async decide<TResult extends JsonValue = JsonValue>(
      contractName: ContractName<C>,
      request: RuntimeDecisionRequest = {},
    ): Promise<RuntimeDecision<TResult>> {
      const binding = config.contracts[contractName];
      if (binding === undefined) {
        throw new MissingContractBindingError(contractName);
      }
      const prepared = cloneRequest(request, config.random);
      const body = JSON.stringify(prepared.input);
      const encodedName = encodeURIComponent(contractName);
      const encodedDigest = encodeURIComponent(binding.contractDigest);
      const url = `${config.decisionServiceUrl}/v3/decision-contracts/`
        + `${encodedName}/versions/${encodedDigest}/decisions`;
      const authorizationValue = await authorization(config.credential);

      for (let attempt = 0; attempt <= config.retries; attempt += 1) {
        let response: Response;
        try {
          response = await config.fetch(url, {
            method: "POST",
            headers: headers(authorizationValue, prepared.correlationId),
            body,
          });
        } catch (error) {
          if (attempt < config.retries) {
            await waitBeforeRetry();
            continue;
          }
          throw error;
        }

        let responseBody: unknown;
        try {
          responseBody = await parseJson(response);
        } catch (error) {
          if (attempt < config.retries && isRetryableStatus(response.status)) {
            await waitBeforeRetry(response);
            continue;
          }
          throw error;
        }

        if (!response.ok) {
          const problem = problemOrThrow(responseBody, response.status);
          if (attempt < config.retries && isRetryableStatus(response.status)) {
            await waitBeforeRetry(response);
            continue;
          }
          throw new FlaggoHttpError(problem, responseMetadata(response));
        }

        return decisionOrThrow<TResult>(responseBody, binding.contractDigest);
      }

      throw new InvalidServerResponseError("Flaggo retry processing ended unexpectedly.");
    },
  };
}

export type { CredentialProvider } from "./wire.js";
