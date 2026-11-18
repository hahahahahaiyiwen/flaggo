import { InvalidServerResponseError } from "../errors.js";
import type { AuthorityScope } from "../configuration/types.js";
import type {
  DecisionContractValidationResult as GeneratedValidationResult,
  DecisionContractVersion as GeneratedDecisionContractVersion,
  DecisionContractVersionList as GeneratedDecisionContractVersionList,
  DecisionContract as GeneratedDecisionContract,
} from "../generated/management-models.generated.js";
import {
  validateDecisionContract,
  validateDecisionContractValidationResult,
  validateDecisionContractVersion,
  validateDecisionContractVersionList,
} from "../generated/management-validators.generated.mjs";
import {
  assertDecisionName,
  assertSha256Digest,
  hasOnlyKeys,
  inputError,
  record,
} from "../internal/guards.js";
import { createTransport } from "../internal/transport.js";
import { resourceScopePath, sameAuthority } from "../internal/resource.js";
import {
  assertInputSchema,
  assertResponseSchema,
} from "../internal/validators.js";
import type { JsonValue, Sha256Digest } from "../shared/types.js";
import type {
  ContractClient,
  ContractClientConfiguration,
  DecisionContractValidationResult,
  DecisionContractVersion,
  DecisionContractVersionList,
  ListContractVersionsQuery,
} from "./types.js";

function validateConfiguration(
  configuration: ContractClientConfiguration,
): void {
  const raw = record(configuration);
  if (
    raw === undefined
    || !hasOnlyKeys(
      raw,
      new Set(["baseUrl", "authority", "fetch", "timeoutMs", "retry"]),
    )
  ) {
    inputError("/", "Contract client configuration contains unknown members.");
  }
}

function contractName(contract: unknown): string {
  const value = record(contract);
  if (value === undefined) {
    inputError("/", "DecisionContract must be a plain object.");
  }
  assertDecisionName(value.name, "/name");
  return value.name;
}

function validationResultOrThrow(
  value: unknown,
): DecisionContractValidationResult {
  assertResponseSchema(
    validateDecisionContractValidationResult,
    value,
    "DecisionContract validation result",
  );
  return value as GeneratedValidationResult as DecisionContractValidationResult;
}

function versionOrThrow<TResult extends JsonValue>(
  value: unknown,
  expectedName: string,
  expectedAuthority: AuthorityScope,
  expectedDigest?: Sha256Digest,
): DecisionContractVersion<TResult> {
  assertResponseSchema(
    validateDecisionContractVersion,
    value,
    "DecisionContract version",
  );
  const version = value as GeneratedDecisionContractVersion;
  if (
    version.name !== expectedName
    || version.contract.name !== expectedName
    || !sameAuthority(version.contract.authority, expectedAuthority)
    || (
      expectedDigest !== undefined
      && version.contractDigest !== expectedDigest
    )
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned a DecisionContract version with mismatched identity.",
    );
  }
  return value as DecisionContractVersion<TResult>;
}

function versionListOrThrow(
  value: unknown,
  expectedName: string,
): DecisionContractVersionList {
  assertResponseSchema(
    validateDecisionContractVersionList,
    value,
    "DecisionContract version list",
  );
  const list = value as GeneratedDecisionContractVersionList;
  if (list.name !== expectedName) {
    throw new InvalidServerResponseError(
      "Flaggo returned a DecisionContract version list with mismatched identity.",
    );
  }
  return value as DecisionContractVersionList;
}

function validateListQuery(
  query: ListContractVersionsQuery,
): URLSearchParams {
  const raw = record(query);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["limit", "cursor"]))
  ) {
    inputError("/query", "Version-list query contains unknown members.");
  }
  if (
    query.limit !== undefined
    && (
      !Number.isInteger(query.limit)
      || query.limit < 1
      || query.limit > 200
    )
  ) {
    inputError("/query/limit", "Limit must be an integer from 1 to 200.");
  }
  if (
    query.cursor !== undefined
    && (
      typeof query.cursor !== "string"
      || query.cursor.length === 0
    )
  ) {
    inputError("/query/cursor", "Cursor must be a non-empty string.");
  }

  const parameters = new URLSearchParams();
  if (query.limit !== undefined) {
    parameters.set("limit", String(query.limit));
  }
  if (query.cursor !== undefined) {
    parameters.set("cursor", query.cursor);
  }
  return parameters;
}

export function createContractClient(
  configuration: ContractClientConfiguration,
): ContractClient {
  validateConfiguration(configuration);
  const scopePath = resourceScopePath(configuration.authority);
  const authority = Object.freeze({ ...configuration.authority });
  const transport = createTransport(configuration);
  function validateContract(value: unknown): void {
    assertInputSchema(validateDecisionContract, value, "DecisionContract");
    if (!sameAuthority((value as GeneratedDecisionContract).authority, authority)) {
      inputError("/authority", "DecisionContract authority must match the client's explicit resource scope.");
    }
  }

  return {
    async validate(contract, options = {}) {
      const name = contractName(contract);
      return transport.request({
        method: "POST",
        path: `${scopePath}/decision-contracts/${encodeURIComponent(name)}/validate`,
        body: contract,
        options,
        successStatuses: [200],
        responseDescription: "contract validation",
        validateBody: validateContract,
        parse: validationResultOrThrow,
      });
    },

    async deploy(contract, options = {}) {
      const name = contractName(contract);
      return transport.request({
        method: "PUT",
        path: `${scopePath}/decision-contracts/${encodeURIComponent(name)}`,
        body: contract,
        options,
        successStatuses: [200, 201],
        responseDescription: "contract deployment",
        validateBody: validateContract,
        parse(value) {
          return versionOrThrow(value, name, authority);
        },
      });
    },

    async getCurrent(contractNameValue, options = {}) {
      assertDecisionName(contractNameValue);
      return transport.request({
        method: "GET",
        path: `${scopePath}/decision-contracts/${encodeURIComponent(contractNameValue)}`,
        options,
        successStatuses: [200],
        responseDescription: "current contract read",
        parse(value) {
          return versionOrThrow(value, contractNameValue, authority);
        },
      });
    },

    async getVersion(contractNameValue, contractDigest, options = {}) {
      assertDecisionName(contractNameValue);
      assertSha256Digest(contractDigest);
      return transport.request({
        method: "GET",
        path: `${scopePath}/decision-contracts/${encodeURIComponent(contractNameValue)}`
          + `/versions/${encodeURIComponent(contractDigest)}`,
        options,
        successStatuses: [200],
        responseDescription: "exact contract-version read",
        parse(value) {
          return versionOrThrow(value, contractNameValue, authority, contractDigest);
        },
      });
    },

    async listVersions(contractNameValue, query = {}, options = {}) {
      assertDecisionName(contractNameValue);
      const parameters = validateListQuery(query);
      const suffix = parameters.size === 0 ? "" : `?${parameters.toString()}`;
      return transport.request({
        method: "GET",
        path: `${scopePath}/decision-contracts/${encodeURIComponent(contractNameValue)}`
          + `/versions${suffix}`,
        options,
        successStatuses: [200],
        responseDescription: "contract-version list",
        parse(value) {
          return versionListOrThrow(value, contractNameValue);
        },
      });
    },
  };
}
