import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  InvalidFlaggoInputError,
  parseFlaggoDeploymentManifest,
  parseFlaggoRuntimeConfiguration,
  parseFlaggoServiceEndpoints,
} from "../src/configuration/index.js";

interface ConfigurationFixture {
  readonly request: { readonly body: unknown };
  readonly expected: { readonly body: unknown };
}

interface NegativeFixture {
  readonly rejectedSamples: readonly {
    readonly body: unknown;
    readonly schemaRef: string;
  }[];
}

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const fixtureRoot = resolve(repositoryRoot, "contracts/fixtures/deployment");

describe("Flaggo configuration", () => {
  it("parses the contract fixture into frozen configuration values", () => {
    const fixture = readFixture<ConfigurationFixture>(
      "01-generate-runtime-configuration.json",
    );

    const manifest = parseFlaggoDeploymentManifest(fixture.request.body);
    const runtimeConfig = parseFlaggoRuntimeConfiguration(
      fixture.expected.body,
    );

    expect(manifest).toEqual(fixture.request.body);
    expect(runtimeConfig).toEqual(fixture.expected.body);
    expect(Object.isFrozen(manifest)).toBe(true);
    expect(Object.isFrozen(manifest.authority)).toBe(true);
    expect(Object.isFrozen(manifest.contracts)).toBe(true);
    expect(Object.isFrozen(runtimeConfig)).toBe(true);
    expect(Object.isFrozen(runtimeConfig.authority)).toBe(true);
    expect(Object.isFrozen(runtimeConfig.services)).toBe(true);
    expect(Object.isFrozen(runtimeConfig.bindings)).toBe(true);
    expect(Object.getPrototypeOf(runtimeConfig.bindings)).toBeNull();
    expect(Object.isFrozen(
      runtimeConfig.bindings["tetris.dropInterval"],
    )).toBe(true);
  });

  it("rejects every deployment contract negative sample", () => {
    const fixture = readFixture<NegativeFixture>(
      "02-invalid-configurations.json",
    );

    for (const sample of fixture.rejectedSamples) {
      const parse = sample.schemaRef.endsWith("/DeploymentManifest")
        ? parseFlaggoDeploymentManifest
        : parseFlaggoRuntimeConfiguration;
      expect(() => parse(sample.body), sample.schemaRef)
        .toThrow(InvalidFlaggoInputError);
    }
  });

  it("validates and normalizes service endpoints independently", () => {
    const endpoints = parseFlaggoServiceEndpoints({
      contractServiceUrl: "https://contracts.test/",
      decisionServiceUrl: "https://decisions.test/api/",
      otlpIngestionUrl: "http://127.0.0.1:4318/",
    });

    expect(endpoints).toEqual({
      contractServiceUrl: "https://contracts.test",
      decisionServiceUrl: "https://decisions.test/api",
      otlpIngestionUrl: "http://127.0.0.1:4318",
    });
    expect(Object.isFrozen(endpoints)).toBe(true);
  });

  if (false) {
    // @ts-expect-error runtime bindings must contain contract binding objects
    parseFlaggoRuntimeConfiguration<number>({});
  }
});

function readFixture<T>(name: string): T {
  return JSON.parse(
    readFileSync(resolve(fixtureRoot, name), "utf8"),
  ) as T;
}
