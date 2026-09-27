import { readFile } from "node:fs/promises";
import {
  basename,
  dirname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from "node:path";

import { createContractClient } from "@flaggo/sdk/management";

const deploymentFormat = "flaggo.deploy/v1";
const maximumContracts = 128;

export async function deployContracts({
  manifestPath,
  baseUrl,
  credential,
  fetch,
  signal,
}) {
  const absoluteManifestPath = resolve(manifestPath);
  const manifestDirectory = dirname(absoluteManifestPath);
  const manifest = await readJson(absoluteManifestPath, signal);
  validateManifest(manifest, absoluteManifestPath);

  const paths = new Set();
  const names = new Set();
  const sources = [];
  for (const contractReference of manifest.contracts) {
    const contractPath = resolveContractPath(
      manifestDirectory,
      contractReference,
    );
    if (paths.has(contractPath)) {
      throw new Error(
        `Flaggo deployment manifest contains duplicate path '${contractReference}'.`,
      );
    }
    paths.add(contractPath);
    const contract = await readJson(contractPath, signal);
    const contractName = contract?.name;
    if (typeof contractName !== "string" || contractName.length === 0) {
      throw new Error(
        `DecisionContract '${contractReference}' must contain a non-empty name.`,
      );
    }
    const expectedFileName = `${contractName}.decision-contract.json`;
    if (basename(contractPath) !== expectedFileName) {
      throw new Error(
        `DecisionContract '${contractReference}' must be named `
        + `'${expectedFileName}' to match contract.name.`,
      );
    }
    if (names.has(contractName)) {
      throw new Error(
        `Flaggo deployment manifest contains duplicate contract '${contractName}'.`,
      );
    }
    names.add(contractName);
    sources.push({ source: contractReference, contract });
  }

  const client = createContractClient({
    baseUrl,
    ...(credential === undefined ? {} : { credential }),
    ...(fetch === undefined ? {} : { fetch }),
  });
  const contracts = [];
  for (const source of sources) {
    const response = await client.deploy(
      source.contract,
      signal === undefined ? {} : { signal },
    );
    contracts.push({
      ...source,
      deployment: response.value,
    });
  }

  return {
    format: deploymentFormat,
    contracts,
  };
}

function validateManifest(manifest, manifestPath) {
  if (
    manifest === null
    || typeof manifest !== "object"
    || Array.isArray(manifest)
  ) {
    throw new Error(`Flaggo deployment manifest '${manifestPath}' must be an object.`);
  }
  const keys = Object.keys(manifest);
  if (
    keys.length !== 2
    || !keys.includes("format")
    || !keys.includes("contracts")
  ) {
    throw new Error(
      `Flaggo deployment manifest '${manifestPath}' must contain only `
      + "'format' and 'contracts'.",
    );
  }
  if (manifest.format !== deploymentFormat) {
    throw new Error(
      `Flaggo deployment manifest '${manifestPath}' must use `
      + `format '${deploymentFormat}'.`,
    );
  }
  if (
    !Array.isArray(manifest.contracts)
    || manifest.contracts.length === 0
    || manifest.contracts.length > maximumContracts
    || manifest.contracts.some((entry) =>
      typeof entry !== "string" || entry.length === 0)
  ) {
    throw new Error(
      `Flaggo deployment manifest '${manifestPath}' must contain 1-`
      + `${maximumContracts} non-empty contract paths.`,
    );
  }
}

function resolveContractPath(manifestDirectory, contractReference) {
  if (
    isAbsolute(contractReference)
    || contractReference.includes("\\")
  ) {
    throw new Error(
      `DecisionContract path '${contractReference}' must be a portable `
      + "forward-slash relative path.",
    );
  }
  const contractPath = resolve(manifestDirectory, contractReference);
  const relativePath = relative(manifestDirectory, contractPath);
  if (
    relativePath === ""
    || relativePath === ".."
    || relativePath.startsWith(`..${sep}`)
    || isAbsolute(relativePath)
  ) {
    throw new Error(
      `DecisionContract path '${contractReference}' must remain within `
      + "the deployment manifest directory.",
    );
  }
  return contractPath;
}

async function readJson(path, signal) {
  let source;
  try {
    source = await readFile(path, {
      encoding: "utf8",
      ...(signal === undefined ? {} : { signal }),
    });
  } catch (error) {
    if (signal?.aborted === true) throw signal.reason ?? error;
    throw new Error(`Unable to read JSON file '${path}'.`, { cause: error });
  }
  try {
    return JSON.parse(source);
  } catch (error) {
    throw new Error(`JSON file '${path}' is invalid.`, { cause: error });
  }
}
