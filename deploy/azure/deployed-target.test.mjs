import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";
import { loadDeployedTarget } from "./deployed-target.mjs";

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

async function fixture(t) {
  const root = await mkdtemp(resolve(tmpdir(), "flaggo-target-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const releaseRoot = resolve(root, "release");
  const stateRoot = resolve(root, "state");
  await Promise.all([mkdir(releaseRoot), mkdir(stateRoot)]);
  await writeFile(resolve(releaseRoot, "binary"), "installed");
  await writeFile(resolve(releaseRoot, "release.json"), JSON.stringify({
    format: "flaggo.release/v1", sourceCommit: "a".repeat(40),
    files: [{ path: "binary", bytes: 9, sha256: hash("installed") }],
  }));
  await writeFile(resolve(stateRoot, ".flaggo-state.json"), JSON.stringify({
    format: "flaggo.state/v1", kind: "disposable-smoke", stateId: "exact-state",
  }));
  const descriptor = {
    format: "flaggo.deployed-smoke/v1", analysis: "reviewed-python-exit",
    disposable: true, stateId: "exact-state", releaseRoot, stateRoot,
    sourceCommit: "a".repeat(40),
    releaseManifestSha256: hash(await readFile(resolve(releaseRoot, "release.json"))),
    contractDigest: `sha256:${"b".repeat(64)}`,
    gatewayUrl: "http://127.0.0.1:4310",
    services: {
      contractServiceUrl: "http://127.0.0.1:8080",
      decisionServiceUrl: "http://127.0.0.1:8081",
      otlpIngestionUrl: "http://127.0.0.1:5090",
    },
    observability: { lokiUrl: "http://127.0.0.1:3100", tempoUrl: "http://127.0.0.1:3200" },
  };
  const path = resolve(root, "target.json");
  await writeFile(path, JSON.stringify(descriptor));
  return { path, descriptor, releaseRoot, stateRoot };
}

test("valid target preserves explicit endpoints and has no shutdown/cleanup handle", async (t) => {
  const { path, stateRoot } = await fixture(t);
  const target = await loadDeployedTarget(path, "exact-state");
  assert.equal(target.runDirectory, await import("node:fs/promises").then((fs) => fs.realpath(stateRoot)));
  assert.equal(target.services.decisionServiceUrl, "http://127.0.0.1:8081");
  assert.equal(target.identity.stateId, "exact-state");
  assert.equal(target.close, undefined);
  assert.equal(target.child, undefined);
});

test("explicit empty/missing target and wrong confirmation fail, never launch a fixture", async (t) => {
  const { path } = await fixture(t);
  await assert.rejects(loadDeployedTarget("", ""), /no local fallback/u);
  await assert.rejects(loadDeployedTarget(resolve(path, "missing"), ""), /ENOENT|ENOTDIR/u);
  await assert.rejects(loadDeployedTarget(path, "different-state"), /Confirm this exact/u);
});

test("release/state drift and non-loopback URLs fail before target use", async (t) => {
  const { path, descriptor, releaseRoot, stateRoot } = await fixture(t);
  descriptor.gatewayUrl = "https://public.example:443";
  await writeFile(path, JSON.stringify(descriptor));
  await assert.rejects(loadDeployedTarget(path, "exact-state"), /loopback/u);
  descriptor.gatewayUrl = "http://127.0.0.1:4310";
  await writeFile(path, JSON.stringify(descriptor));
  await writeFile(resolve(releaseRoot, "binary"), "changed!!");
  await assert.rejects(loadDeployedTarget(path, "exact-state"), /file mismatch/u);
  await writeFile(resolve(releaseRoot, "binary"), "installed");
  await writeFile(resolve(stateRoot, ".flaggo-state.json"), JSON.stringify({
    format: "flaggo.state/v1", kind: "persistent", stateId: "exact-state",
  }));
  await assert.rejects(loadDeployedTarget(path, "exact-state"), /state identity/u);
});
