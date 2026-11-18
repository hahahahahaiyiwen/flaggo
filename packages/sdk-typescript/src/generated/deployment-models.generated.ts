/*
 * Generated from Flaggo JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */

/**
 * Authored deployment manifest and generated immutable runtime configuration.
 */
export type FlaggoDeploymentModelsV3 = DeploymentManifest | RuntimeConfiguration;
export type AuthorityIdentifier = string;
/**
 * Portable ASCII forward-slash relative path contained by the deployment manifest directory.
 */
export type DecisionContractPath = string;
/**
 * Absolute HTTP(S) service base URL with an optional port from 1 through 65535 and without credentials, query, or fragment.
 */
export type ServiceUrl = string;
export type Sha256Digest = string;

export interface DeploymentManifest {
  format: "flaggo.deploy/v2";
  authority: AuthorityScope;
  /**
   * Authority-free authored DecisionContracts that deployment binds to the manifest authority before validation, digesting, and persistence.
   *
   * @minItems 1
   * @maxItems 128
   */
  contracts: DecisionContractPath[];
}
/**
 * Declared routing authority injected into every deployed DecisionContract and application telemetry signal in one deployment.
 */
export interface AuthorityScope {
  tenant: AuthorityIdentifier;
  application: AuthorityIdentifier;
  environment: AuthorityIdentifier;
}
/**
 * Generated immutable v4 client configuration. The explicit authority selects tenant/application/environment routes and is not an authentication credential. Legacy runtime-config/v1 must be explicitly regenerated.
 */
export interface RuntimeConfiguration {
  format: "flaggo.runtime-config/v2";
  authority: AuthorityScope;
  services: ServiceEndpoints;
  bindings: {
    [k: string]: ContractBinding;
  };
}
export interface ServiceEndpoints {
  contractServiceUrl: ServiceUrl;
  decisionServiceUrl: ServiceUrl;
  otlpIngestionUrl: ServiceUrl;
}
export interface ContractBinding {
  contractDigest: Sha256Digest;
}
