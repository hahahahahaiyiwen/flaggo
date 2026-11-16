import { createHash } from "node:crypto";
import { readFile, readlink, realpath, stat } from "node:fs/promises";
import { isAbsolute, resolve, sep } from "node:path";

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const sha256 = /^[0-9a-f]{64}$/u;
const digest = /^sha256:[0-9a-f]{64}$/u;
const readJson = async (path) => JSON.parse(await readFile(path, "utf8"));

function loopback(value) {
  const url = new URL(value);
  if (url.protocol !== "http:" || !["127.0.0.1", "[::1]"].includes(url.hostname)
    || url.username || url.password || url.search || url.hash
    || url.pathname !== "/" || !url.port) {
    throw new Error("Deployed smoke requires explicit loopback HTTP endpoints with ports.");
  }
  return url.origin;
}

export async function loadDeployedTarget(path, confirmation) {
  if (!path || !isAbsolute(path)) {
    throw new Error("FLAGGO_DEPLOYED_TARGET must name an absolute descriptor file; no local fallback.");
  }
  const target = await readJson(path);
  if (target.format !== "flaggo.deployed-smoke/v1"
    || target.analysis !== "reviewed-python-exit" || target.disposable !== true
    || typeof target.stateId !== "string" || !target.stateId
    || confirmation !== target.stateId) {
    throw new Error("Confirm this exact disposable reviewed-python-exit target with FLAGGO_DEPLOYED_SMOKE_CONFIRM.");
  }
  if (!isAbsolute(target.releaseRoot) || !isAbsolute(target.stateRoot)
    || !sha256.test(target.releaseManifestSha256) || !digest.test(target.contractDigest)) {
    throw new Error("Deployed target requires absolute paths and exact release/contract identities.");
  }
  const stateRoot = await realpath(target.stateRoot);
  const state = await readJson(resolve(stateRoot, ".flaggo-state.json"));
  if (state.format !== "flaggo.state/v1" || state.kind !== "disposable-smoke"
    || state.stateId !== target.stateId) {
    throw new Error("Mounted state identity differs from the disposable smoke descriptor.");
  }
  const releaseRoot = await realpath(target.releaseRoot);
  const bytes = await readFile(resolve(releaseRoot, "release.json"));
  if (hash(bytes) !== target.releaseManifestSha256) {
    throw new Error("Installed release manifest hash mismatch.");
  }
  const manifest = JSON.parse(bytes.toString("utf8"));
  if (manifest.format !== "flaggo.release/v1"
    || !/^[0-9a-f]{40}$/u.test(manifest.sourceCommit)
    || manifest.sourceCommit !== target.sourceCommit || !Array.isArray(manifest.files)
    || manifest.files.length === 0) {
    throw new Error("Installed release/source identity is invalid.");
  }
  for (const file of manifest.files) {
    if (typeof file.path !== "string" || isAbsolute(file.path)
      || file.path.split(/[\\/]/u).includes("..") || !sha256.test(file.sha256)
      || !Number.isSafeInteger(file.bytes) || file.bytes < 0) {
      throw new Error("Invalid installed release file manifest.");
    }
    for (const link of manifest.symlinks ?? []) {
      if (typeof link.path !== "string" || isAbsolute(link.path)
        || link.path.split(/[\\/]/u).includes("..")
        || await readlink(resolve(releaseRoot, link.path)) !== link.target
        || !(await realpath(resolve(releaseRoot, link.path))).startsWith(`${releaseRoot}${sep}`)) {
        throw new Error("Installed release symlink mismatch.");
      }
    }
    const installed = await realpath(resolve(releaseRoot, file.path));
    if (!installed.startsWith(`${releaseRoot}${sep}`)
      || (await stat(installed)).size !== file.bytes
      || hash(await readFile(installed)) !== file.sha256) {
      throw new Error(`Installed release file mismatch: ${file.path}`);
    }
  }
  return {
    url: loopback(target.gatewayUrl),
    runDirectory: stateRoot,
    services: Object.fromEntries(
      ["contractServiceUrl", "decisionServiceUrl", "otlpIngestionUrl"]
        .map((key) => [key, loopback(target.services[key])]),
    ),
    observability: Object.fromEntries(
      ["lokiUrl", "tempoUrl"].map((key) => [key, loopback(target.observability[key])]),
    ),
    identity: {
      stateId: target.stateId, contractDigest: target.contractDigest,
      sourceCommit: manifest.sourceCommit,
      releaseManifestSha256: target.releaseManifestSha256,
    },
  };
}
