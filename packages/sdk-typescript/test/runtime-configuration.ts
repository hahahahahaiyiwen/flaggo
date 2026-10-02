import {
  parseFlaggoRuntimeConfiguration,
  type AuthorityScope,
  type FlaggoRuntimeConfiguration,
  type RuntimeBindings,
} from "../src/configuration/index.js";

const defaultAuthority: AuthorityScope = {
  tenant: "local",
  application: "sdk-test",
  environment: "test",
};

export function runtimeConfiguration<TBindings extends RuntimeBindings>(
  bindings: TBindings,
  options: {
    readonly authority?: AuthorityScope;
    readonly contractServiceUrl?: string;
    readonly decisionServiceUrl?: string;
    readonly otlpIngestionUrl?: string;
  } = {},
): FlaggoRuntimeConfiguration<TBindings> {
  return parseFlaggoRuntimeConfiguration<TBindings>({
    format: "flaggo.runtime-config/v1",
    authority: options.authority ?? defaultAuthority,
    services: {
      contractServiceUrl:
        options.contractServiceUrl ?? "https://contracts.test",
      decisionServiceUrl:
        options.decisionServiceUrl ?? "https://decisions.test",
      otlpIngestionUrl:
        options.otlpIngestionUrl ?? "https://telemetry.test",
    },
    bindings,
  });
}
