import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { canonicalize } from "json-canonicalize";
import { describe, expect, it } from "vitest";

import { compileManifest, contractDigest, parseManifest } from "../src/manifest.js";
import { runManifestCli } from "../src/manifest-cli.js";
import { gauge, manifest } from "./support.js";

describe("manifest compilation", () => {
  it("normalizes one contract and emits exact full digests without author IDs", () => {
    const input = manifest();
    const result = compileManifest(input);
    const contract = { ...input.decisions.parallelism, context: {} };
    const expected = `sha256:${createHash("sha256")
      .update(canonicalize({ key: "parallelism", contract })).digest("hex")}`;
    expect(result.catalog.decisions.parallelism?.contractDigest).toBe(expected);
    expect(result.bundle.decisions.parallelism?.context).toEqual({});
    expect(result.catalogSource).toContain("as const satisfies RuntimeCatalog");
    expect(result.catalogSource).not.toContain("flaggo.signal");
  });

  it("hashes semantic bindings but not owner or publication metadata", () => {
    const original = manifest();
    const first = compileManifest(original);
    const changed = structuredClone(original);
    const definition = changed.decisions.parallelism!;
    definition.owner = "operations";
    changed.source = { commit: "different-build" };
    const metadata = compileManifest(changed);
    expect(metadata.catalog.decisions.parallelism?.contractDigest)
      .toBe(first.catalog.decisions.parallelism?.contractDigest);
    expect(metadata.catalog.bundleDigest).not.toBe(first.catalog.bundleDigest);
    definition.evidence = { occupancy: { ...gauge, meaning: "A different measurement." } };
    expect(contractDigest("parallelism", definition)).not.toBe(first.catalog.decisions.parallelism?.contractDigest);
    expect(contractDigest("other-key", original.decisions.parallelism!))
      .not.toBe(first.catalog.decisions.parallelism?.contractDigest);
  });

  it.each([
    ['"format": "flaggo.decision-definition-bundle/v2"', '"format": "flaggo.decision-definition-bundle/v1"'],
    ['"maxAgeSeconds": 30', '"maxAgeSeconds": 0'],
    ['"dataType": "gauge"', '"dataType": "sum"'],
    ['"kind": "latest"', '"kind": "average"'],
    ['"accept": "observed"', '"accept": "complete"'],
    ['"binding": "occupancy"', '"binding": "missing"'],
    ['"maxAgeSeconds": 30', '"maxAgeSeconds": 922337203686'],
  ])("rejects unsupported or invalid semantics: %s", (before, after) => {
    const text = JSON.stringify(manifest(), null, 2);
    expect(() => parseManifest(text.replace(before, after))).toThrow();
  });

  it("rejects duplicate properties and lossy JSON numbers before canonicalization", () => {
    const text = JSON.stringify(manifest());
    expect(() => parseManifest(text.replace('"default":2', '"default":2,"default":3')))
      .toThrow("Duplicate JSON property");
    expect(() => parseManifest(text.replace('"max":10', '"max":9007199254740993')))
      .toThrow("round-trip");
  });

  it("checks generated artifacts without rewriting stale outputs", async () => {
    const directory = await mkdtemp(join(tmpdir(), "flaggo-manifest-"));
    try {
      const source = join(directory, "manifest.json");
      const bundle = join(directory, "bundle.json");
      const catalog = join(directory, "catalog.ts");
      await writeFile(source, JSON.stringify(manifest()));
      const args = [source, "--bundle", bundle, "--catalog", catalog];
      await runManifestCli(["build", ...args]);
      await runManifestCli(["check", ...args]);
      await writeFile(catalog, "stale");
      await expect(runManifestCli(["check", ...args])).rejects.toThrow("stale");
      expect(await readFile(catalog, "utf8")).toBe("stale");
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
