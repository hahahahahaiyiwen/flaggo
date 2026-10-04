import type {
  AuthorityScope as GeneratedAuthorityScope,
  ContractBinding as GeneratedContractBinding,
  DeploymentManifest as GeneratedDeploymentManifest,
  RuntimeConfiguration as GeneratedRuntimeConfiguration,
  ServiceEndpoints as GeneratedServiceEndpoints,
} from "../generated/deployment-models.generated.js";
import type { DeepReadonly, Sha256Digest } from "../shared/types.js";

export type AuthorityScope = DeepReadonly<GeneratedAuthorityScope>;
export type DeploymentManifest = DeepReadonly<GeneratedDeploymentManifest>;
export type ServiceEndpoints = DeepReadonly<GeneratedServiceEndpoints>;

export type RuntimeContractBinding =
  Omit<DeepReadonly<GeneratedContractBinding>, "contractDigest"> & {
    readonly contractDigest: Sha256Digest;
  };

export type RuntimeBindings =
  Readonly<Record<string, RuntimeContractBinding>>;

export type FlaggoRuntimeConfiguration<
  TBindings extends RuntimeBindings = RuntimeBindings,
> = Omit<
  DeepReadonly<GeneratedRuntimeConfiguration>,
  "bindings"
> & {
  readonly bindings: TBindings;
};
