import type {
  DeploymentManifest as GeneratedDeploymentManifest,
  RuntimeConfiguration as GeneratedRuntimeConfiguration,
  ServiceEndpoints as GeneratedServiceEndpoints,
} from "../generated/deployment-models.generated.js";
import {
  validateDeploymentManifest,
  validateRuntimeConfiguration,
  validateServiceEndpoints,
} from "../generated/deployment-validators.generated.mjs";
import { normalizeServiceBaseUrl } from "../internal/service-url.js";
import { assertInputSchema } from "../internal/validators.js";
import type { Sha256Digest } from "../shared/types.js";
import type {
  AuthorityScope,
  DeploymentManifest,
  FlaggoRuntimeConfiguration,
  RuntimeBindings,
  RuntimeContractBinding,
  ServiceEndpoints,
} from "./types.js";

export function parseFlaggoDeploymentManifest(
  value: unknown,
): DeploymentManifest {
  assertInputSchema(
    validateDeploymentManifest,
    value,
    "Flaggo deployment manifest",
  );
  const manifest = value as GeneratedDeploymentManifest;
  return Object.freeze({
    format: manifest.format,
    authority: freezeAuthority(manifest.authority),
    contracts: Object.freeze([...manifest.contracts]),
  });
}

export function parseFlaggoRuntimeConfiguration<
  TBindings extends RuntimeBindings = RuntimeBindings,
>(
  value: unknown,
): FlaggoRuntimeConfiguration<TBindings> {
  assertInputSchema(
    validateRuntimeConfiguration,
    value,
    "Flaggo runtime configuration",
  );
  const configuration = value as GeneratedRuntimeConfiguration;
  const bindings = Object.create(null) as Record<
    string,
    RuntimeContractBinding
  >;
  for (const [name, binding] of Object.entries(configuration.bindings)) {
    bindings[name] = Object.freeze({
      contractDigest: binding.contractDigest as Sha256Digest,
    });
  }
  return Object.freeze({
    format: configuration.format,
    authority: freezeAuthority(configuration.authority),
    services: freezeServices(configuration.services),
    bindings: Object.freeze(bindings) as TBindings,
  });
}

export function parseFlaggoServiceEndpoints(
  value: unknown,
): ServiceEndpoints {
  assertInputSchema(
    validateServiceEndpoints,
    value,
    "Flaggo service endpoints",
  );
  return freezeServices(value as GeneratedServiceEndpoints);
}

function freezeAuthority(value: GeneratedDeploymentManifest["authority"]):
  AuthorityScope {
  return Object.freeze({
    tenant: value.tenant,
    application: value.application,
    environment: value.environment,
  });
}

function freezeServices(
  value: GeneratedRuntimeConfiguration["services"],
): ServiceEndpoints {
  return Object.freeze({
    contractServiceUrl: normalizeServiceBaseUrl(
      value.contractServiceUrl,
      "/services/contractServiceUrl",
    ),
    decisionServiceUrl: normalizeServiceBaseUrl(
      value.decisionServiceUrl,
      "/services/decisionServiceUrl",
    ),
    otlpIngestionUrl: normalizeServiceBaseUrl(
      value.otlpIngestionUrl,
      "/services/otlpIngestionUrl",
    ),
  });
}
