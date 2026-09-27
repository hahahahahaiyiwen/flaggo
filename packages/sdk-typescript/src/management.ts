import {
  FlaggoHttpError,
  InvalidDecisionInputError,
  InvalidServerResponseError,
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
  DecisionContract,
  DecisionContractValidationResult,
  DecisionContractVersion,
  DecisionContractVersionList,
  FetchLike,
  JsonValue,
  Sha256Digest,
} from "./types.js";

export interface ContractServiceClientConfig {
  contractServiceUrl: string;
  credential?: CredentialProvider;
  fetch?: FetchLike;
}

export interface ManagementRequestOptions {
  correlationId?: string;
}

export interface ListContractVersionsOptions extends ManagementRequestOptions {
  limit?: number;
  cursor?: string;
}

export interface ContractServiceClient {
  validate<TResult extends JsonValue = JsonValue>(
    contractName: string,
    contract: DecisionContract<TResult>,
    options?: ManagementRequestOptions,
  ): Promise<DecisionContractValidationResult>;

  put<TResult extends JsonValue = JsonValue>(
    contractName: string,
    contract: DecisionContract<TResult>,
    options?: ManagementRequestOptions,
  ): Promise<DecisionContractVersion<TResult>>;

  getCurrent<TResult extends JsonValue = JsonValue>(
    contractName: string,
    options?: ManagementRequestOptions,
  ): Promise<DecisionContractVersion<TResult>>;

  getVersion<TResult extends JsonValue = JsonValue>(
    contractName: string,
    contractDigest: Sha256Digest,
    options?: ManagementRequestOptions,
  ): Promise<DecisionContractVersion<TResult>>;

  listVersions(
    contractName: string,
    options?: ListContractVersionsOptions,
  ): Promise<DecisionContractVersionList>;
}

function validateOptions(options: ManagementRequestOptions): void {
  const value = record(options);
  if (
    value === undefined
    || !hasOnlyKeys(value, new Set(["correlationId"]))
    || (
      options.correlationId !== undefined
      && (
        options.correlationId.length === 0
        || options.correlationId.length > 256
        || /[\r\n]/u.test(options.correlationId)
      )
    )
  ) {
    throw new InvalidDecisionInputError(
      "/correlationId",
      "Correlation IDs must contain 1-256 characters without line breaks.",
    );
  }
}

function validateContractIdentity(
  contractName: string,
  contract?: DecisionContract,
): void {
  if (!isDecisionName(contractName)) {
    throw new InvalidDecisionInputError(
      "/contractName",
      "Contract name is invalid.",
    );
  }
  if (contract !== undefined && contract.name !== contractName) {
    throw new InvalidDecisionInputError(
      "/name",
      "The contract name must match the route contractName.",
    );
  }
}

function isTimestamp(value: unknown): value is string {
  return typeof value === "string"
    && value.endsWith("Z")
    && !Number.isNaN(Date.parse(value));
}

function isValidationIssue(value: unknown): boolean {
  const issue = record(value);
  return issue !== undefined
    && hasOnlyKeys(issue, new Set(["code", "severity", "path", "message"]))
    && typeof issue.code === "string"
    && /^[a-z][a-z0-9-]*$/u.test(issue.code)
    && (issue.severity === "error" || issue.severity === "warning")
    && typeof issue.path === "string"
    && typeof issue.message === "string"
    && issue.message.length > 0;
}

function validationResultOrThrow(
  value: unknown,
): DecisionContractValidationResult {
  const result = record(value);
  if (
    result === undefined
    || !hasOnlyKeys(result, new Set(["status", "contractDigest", "issues"]))
    || !Array.isArray(result.issues)
    || !result.issues.every(isValidationIssue)
    || (
      result.status === "valid"
      && (
        !isSha256Digest(result.contractDigest)
        || result.issues.some(
          (issue) => record(issue)?.severity !== "warning",
        )
      )
    )
    || (
      result.status === "invalid"
      && (
        result.contractDigest !== undefined
        || !result.issues.some(
          (issue) => record(issue)?.severity === "error",
        )
      )
    )
    || (result.status !== "valid" && result.status !== "invalid")
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned a malformed DecisionContract validation result.",
    );
  }
  return structuredClone(value) as DecisionContractValidationResult;
}

function versionOrThrow<TResult extends JsonValue>(
  value: unknown,
  expectedName: string,
  expectedDigest?: Sha256Digest,
): DecisionContractVersion<TResult> {
  const version = record(value);
  const contract = record(version?.contract);
  if (
    version === undefined
    || !hasOnlyKeys(
      version,
      new Set([
        "name",
        "contractDigest",
        "status",
        "acceptedAt",
        "activeExecutableDigest",
        "contract",
      ]),
    )
    || version.name !== expectedName
    || !isSha256Digest(version.contractDigest)
    || (expectedDigest !== undefined && version.contractDigest !== expectedDigest)
    || version.status !== "ready"
    || !isTimestamp(version.acceptedAt)
    || !isSha256Digest(version.activeExecutableDigest)
    || contract === undefined
    || contract.name !== expectedName
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned a malformed DecisionContract version.",
    );
  }
  return structuredClone(value) as DecisionContractVersion<TResult>;
}

function versionListOrThrow(
  value: unknown,
  expectedName: string,
): DecisionContractVersionList {
  const list = record(value);
  if (
    list === undefined
    || !hasOnlyKeys(
      list,
      new Set(["name", "currentContractDigest", "versions", "nextCursor"]),
    )
    || list.name !== expectedName
    || !isSha256Digest(list.currentContractDigest)
    || !Array.isArray(list.versions)
    || (
      list.nextCursor !== null
      && (typeof list.nextCursor !== "string" || list.nextCursor.length === 0)
    )
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned a malformed DecisionContract version list.",
    );
  }
  for (const item of list.versions) {
    const version = record(item);
    if (
      version === undefined
      || !hasOnlyKeys(
        version,
        new Set([
          "contractDigest",
          "status",
          "acceptedAt",
          "activeExecutableDigest",
        ]),
      )
      || !isSha256Digest(version.contractDigest)
      || version.status !== "ready"
      || !isTimestamp(version.acceptedAt)
      || !isSha256Digest(version.activeExecutableDigest)
    ) {
      throw new InvalidServerResponseError(
        "Flaggo returned a malformed DecisionContract version summary.",
      );
    }
  }
  return structuredClone(value) as DecisionContractVersionList;
}

export function createContractServiceClient(
  configuration: ContractServiceClientConfig,
): ContractServiceClient {
  const raw = record(configuration);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["contractServiceUrl", "credential", "fetch"]))
    || typeof configuration.contractServiceUrl !== "string"
    || !URL.canParse(configuration.contractServiceUrl)
    || (configuration.fetch !== undefined && typeof configuration.fetch !== "function")
  ) {
    throw new InvalidDecisionInputError(
      "/",
      "Contract Service client configuration is invalid.",
    );
  }
  const baseUrl = configuration.contractServiceUrl.replace(/\/+$/u, "");
  const fetch = configuration.fetch ?? globalThis.fetch.bind(globalThis);

  async function send(
    method: string,
    path: string,
    options: ManagementRequestOptions,
    body?: DecisionContract,
  ): Promise<{ response: Response; body: unknown }> {
    validateOptions(options);
    const response = await fetch(`${baseUrl}${path}`, {
      method,
      headers: headers(
        await authorization(configuration.credential),
        options.correlationId,
      ),
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    const responseBody = await parseJson(response);
    if (!response.ok) {
      throw new FlaggoHttpError(
        problemOrThrow(responseBody, response.status),
        responseMetadata(response),
      );
    }
    return { response, body: responseBody };
  }

  return {
    async validate<TResult extends JsonValue = JsonValue>(
      contractName: string,
      contract: DecisionContract<TResult>,
      options: ManagementRequestOptions = {},
    ): Promise<DecisionContractValidationResult> {
      validateContractIdentity(contractName, contract);
      const result = await send(
        "POST",
        `/v3/decision-contracts/${encodeURIComponent(contractName)}/validate`,
        options,
        contract,
      );
      if (result.response.status !== 200) {
        throw new InvalidServerResponseError(
          `Flaggo returned unexpected HTTP ${result.response.status} for validation.`,
        );
      }
      return validationResultOrThrow(result.body);
    },

    async put<TResult extends JsonValue = JsonValue>(
      contractName: string,
      contract: DecisionContract<TResult>,
      options: ManagementRequestOptions = {},
    ): Promise<DecisionContractVersion<TResult>> {
      validateContractIdentity(contractName, contract);
      const result = await send(
        "PUT",
        `/v3/decision-contracts/${encodeURIComponent(contractName)}`,
        options,
        contract,
      );
      if (result.response.status !== 200 && result.response.status !== 201) {
        throw new InvalidServerResponseError(
          `Flaggo returned unexpected HTTP ${result.response.status} for publication.`,
        );
      }
      return versionOrThrow<TResult>(result.body, contractName);
    },

    async getCurrent<TResult extends JsonValue = JsonValue>(
      contractName: string,
      options: ManagementRequestOptions = {},
    ): Promise<DecisionContractVersion<TResult>> {
      validateContractIdentity(contractName);
      const result = await send(
        "GET",
        `/v3/decision-contracts/${encodeURIComponent(contractName)}`,
        options,
      );
      return versionOrThrow<TResult>(result.body, contractName);
    },

    async getVersion<TResult extends JsonValue = JsonValue>(
      contractName: string,
      contractDigest: Sha256Digest,
      options: ManagementRequestOptions = {},
    ): Promise<DecisionContractVersion<TResult>> {
      validateContractIdentity(contractName);
      if (!isSha256Digest(contractDigest)) {
        throw new InvalidDecisionInputError(
          "/contractDigest",
          "Contract digest must use sha256:<lowercase-hex>.",
        );
      }
      const result = await send(
        "GET",
        `/v3/decision-contracts/${encodeURIComponent(contractName)}`
          + `/versions/${encodeURIComponent(contractDigest)}`,
        options,
      );
      return versionOrThrow<TResult>(
        result.body,
        contractName,
        contractDigest,
      );
    },

    async listVersions(
      contractName: string,
      options: ListContractVersionsOptions = {},
    ): Promise<DecisionContractVersionList> {
      validateContractIdentity(contractName);
      const rawOptions = record(options);
      if (
        rawOptions === undefined
        || !hasOnlyKeys(rawOptions, new Set(["limit", "cursor", "correlationId"]))
        || (
          options.limit !== undefined
          && (
            !Number.isInteger(options.limit)
            || options.limit < 1
            || options.limit > 200
          )
        )
        || (
          options.cursor !== undefined
          && options.cursor.length === 0
        )
      ) {
        throw new InvalidDecisionInputError(
          "/",
          "Version-list options are invalid.",
        );
      }
      validateOptions(
        options.correlationId === undefined
          ? {}
          : { correlationId: options.correlationId },
      );
      const query = new URLSearchParams();
      if (options.limit !== undefined) query.set("limit", String(options.limit));
      if (options.cursor !== undefined) query.set("cursor", options.cursor);
      const suffix = query.size === 0 ? "" : `?${query.toString()}`;
      const result = await send(
        "GET",
        `/v3/decision-contracts/${encodeURIComponent(contractName)}/versions${suffix}`,
        options.correlationId === undefined
          ? {}
          : { correlationId: options.correlationId },
      );
      return versionListOrThrow(result.body, contractName);
    },
  };
}

export type { CredentialProvider } from "./wire.js";
