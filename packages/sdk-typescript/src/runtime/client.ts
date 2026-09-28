import {
  InvalidServerResponseError,
  MissingDecisionBindingError,
} from "../errors.js";
import type { RuntimeDecision as GeneratedRuntimeDecision } from "../generated/runtime-models.generated.js";
import {
  validateRuntimeDecision,
  validateRuntimeInput,
} from "../generated/runtime-validators.generated.mjs";
import {
  assertDecisionName,
  assertSha256Digest,
  hasOnlyKeys,
  inputError,
  record,
} from "../internal/guards.js";
import { utf8Length } from "../internal/serialization.js";
import { createTransport } from "../internal/transport.js";
import {
  assertInputSchema,
  assertResponseSchema,
} from "../internal/validators.js";
import type { JsonValue, Sha256Digest } from "../shared/types.js";
import type {
  DecisionBinding,
  DecisionCatalog,
  DecisionClient,
  DecisionClientConfiguration,
  DecisionRequest,
  DecisionSpec,
  RuntimeDecision,
} from "./types.js";

const attributeNamePattern = /^[A-Za-z][A-Za-z0-9_]{0,127}$/u;

function validateConfiguration<TCatalog extends DecisionCatalog>(
  configuration: DecisionClientConfiguration<TCatalog>,
): Readonly<Record<string, DecisionBinding>> {
  const raw = record(configuration);
  if (
    raw === undefined
    || !hasOnlyKeys(
      raw,
      new Set([
        "baseUrl",
        "bindings",
        "credential",
        "fetch",
        "timeoutMs",
        "retry",
        "random",
      ]),
    )
  ) {
    inputError("/", "Decision client configuration contains unknown members.");
  }
  if (configuration.random !== undefined && typeof configuration.random !== "function") {
    inputError("/random", "Random must be a function.");
  }

  const bindings = record(configuration.bindings);
  if (bindings === undefined || Object.keys(bindings).length === 0) {
    inputError("/bindings", "At least one decision binding is required.");
  }
  const clone: Record<string, DecisionBinding> = Object.create(null);
  for (const [contractName, rawBinding] of Object.entries(bindings)) {
    assertDecisionName(contractName, `/bindings/${contractName}`);
    const binding = record(rawBinding);
    if (
      binding === undefined
      || !hasOnlyKeys(binding, new Set(["contractDigest"]))
    ) {
      inputError(
        `/bindings/${contractName}`,
        "Each decision binding must contain only contractDigest.",
      );
    }
    assertSha256Digest(
      binding.contractDigest,
      `/bindings/${contractName}/contractDigest`,
    );
    clone[contractName] = {
      contractDigest: binding.contractDigest,
    };
  }
  return clone;
}

function completeInput<TSpec extends DecisionSpec<
  Readonly<Record<string, JsonValue>>,
  JsonValue
>>(
  request: DecisionRequest<TSpec>,
  random: () => number,
): unknown {
  const raw = record(request);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["attributes", "currentExposure"]))
  ) {
    inputError("/", "Decision request contains unknown members.");
  }
  const attributes = request.attributes ?? {};
  const rawAttributes = record(attributes);
  if (rawAttributes === undefined) {
    inputError("/attributes", "Attributes must be a plain object.");
  }
  if (Object.keys(rawAttributes).length > 128) {
    inputError("/attributes", "At most 128 application attributes may be supplied.");
  }

  const completeAttributes: Record<string, unknown> = Object.create(null);
  for (const [name, value] of Object.entries(rawAttributes)) {
    if (!attributeNamePattern.test(name)) {
      inputError(
        `/attributes/${name}`,
        "Attribute names must begin with a letter and contain only letters, digits, and underscores; underscore-prefixed names are reserved.",
      );
    }
    completeAttributes[name] = value;
  }

  const randomValue = random();
  if (!Number.isFinite(randomValue) || randomValue < 0 || randomValue >= 1) {
    inputError(
      "/attributes/_random",
      "The SDK random source must return a finite number in [0, 1).",
    );
  }
  completeAttributes._random = randomValue;

  if (request.currentExposure !== undefined) {
    const exposure = record(request.currentExposure);
    if (
      exposure === undefined
      || !hasOnlyKeys(exposure, new Set(["exposureId"]))
      || typeof exposure.exposureId !== "string"
      || exposure.exposureId.length === 0
      || utf8Length(exposure.exposureId) > 256
    ) {
      inputError(
        "/currentExposure/exposureId",
        "Exposure IDs must contain 1-256 UTF-8 bytes.",
      );
    }
  }

  return {
    attributes: completeAttributes,
    ...(request.currentExposure === undefined
      ? {}
      : { currentExposure: request.currentExposure }),
  };
}

function decisionOrThrow<TResult extends JsonValue>(
  value: unknown,
  expectedDigest: Sha256Digest,
): RuntimeDecision<TResult> {
  assertResponseSchema(
    validateRuntimeDecision,
    value,
    "RuntimeDecision",
  );
  const decision = value as GeneratedRuntimeDecision;
  if (decision.contractDigest !== expectedDigest) {
    throw new InvalidServerResponseError(
      "Flaggo returned a RuntimeDecision for a different contract digest.",
    );
  }
  return value as RuntimeDecision<TResult>;
}

export function createDecisionClient<TCatalog extends DecisionCatalog>(
  configuration: DecisionClientConfiguration<TCatalog>,
): DecisionClient<TCatalog> {
  const bindings = validateConfiguration(configuration);
  const random = configuration.random ?? Math.random;
  const transport = createTransport(configuration);

  return {
    async decide(contractName, request = {}, options = {}) {
      const binding = bindings[contractName];
      if (binding === undefined) {
        throw new MissingDecisionBindingError(contractName);
      }
      const input = completeInput(request, random);
      const encodedName = encodeURIComponent(contractName);
      const encodedDigest = encodeURIComponent(binding.contractDigest);
      return transport.request({
        method: "POST",
        path: `/v3/decision-contracts/${encodedName}/versions/${encodedDigest}/decisions`,
        body: input,
        options,
        successStatuses: [200],
        responseDescription: "runtime evaluation",
        validateBody(value) {
          assertInputSchema(validateRuntimeInput, value, "RuntimeInput");
        },
        parse(value) {
          return decisionOrThrow(value, binding.contractDigest);
        },
      });
    },
  };
}
