import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import { bundleDigest, contractDigest, normalizeBundle } from "../src/canonical.js";
import { compileManifest } from "../src/manifest.js";
import type { DecisionDefinition, DecisionDefinitionBundle } from "../src/types.js";

interface Vectors {
  definitionCases: { name: string; key: string; variants: { definition: DecisionDefinition }[]; expectedContractDigest: string }[];
  inequivalentDefinitionCases: {
    name: string;
    left: { key: string; definition: DecisionDefinition };
    right: { key: string; definition: DecisionDefinition };
    expectedLeftDigest: string; expectedRightDigest: string;
  }[];
  bundleCases: { name: string; variants: { bundle: DecisionDefinitionBundle }[]; expectedBundleDigest: string }[];
  normalizationErrorCases: { name: string; bundle: DecisionDefinitionBundle }[];
  definitionValidationCases: { name: string; definition: DecisionDefinition; expectedErrors: string[] }[];
}
const vectors = JSON.parse(readFileSync(resolve(import.meta.dirname,
  "../../../contracts/conformance/semantic-digest-vectors-v1.json"), "utf8")) as Vectors;

for (const item of vectors.definitionValidationCases) {
  it(`validates shared evidence ownership semantics: ${item.name}`, () => {
    const compile = () => compileManifest({
      format: "flaggo.decision-definition-bundle/v2", application: { id: "worker", environment: "test" },
      decisions: { pressure: item.definition },
    });
    if (item.expectedErrors.length === 0) expect(compile).not.toThrow();
    else expect(compile).toThrow("Confirmed-exposure bindings are outcome evidence");
  });
}

for (const item of vectors.definitionCases) {
  it(`agrees with the shared semantic contract: ${item.name}`, () => {
    for (const variant of item.variants)
      expect(contractDigest(item.key, variant.definition)).toBe(item.expectedContractDigest);
  });
}
for (const item of vectors.inequivalentDefinitionCases) {
  it(`retains the semantic distinction: ${item.name}`, () => {
    expect(contractDigest(item.left.key, item.left.definition)).toBe(item.expectedLeftDigest);
    expect(contractDigest(item.right.key, item.right.definition)).toBe(item.expectedRightDigest);
    expect(item.expectedLeftDigest).not.toBe(item.expectedRightDigest);
  });
}
for (const item of vectors.bundleCases) {
  it(`agrees with canonical publication identity: ${item.name}`, () => {
    for (const variant of item.variants)
      expect(bundleDigest(variant.bundle)).toBe(item.expectedBundleDigest);
  });
}
for (const item of vectors.normalizationErrorCases) {
  it(`rejects noncanonical authoring: ${item.name}`, () => {
    expect(() => normalizeBundle(item.bundle)).toThrow();
  });
}
