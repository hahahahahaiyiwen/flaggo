import { readFile } from "node:fs/promises";
import {
  basename,
  dirname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from "node:path";

import {
  parseFlaggoDeploymentManifest,
  parseFlaggoRuntimeConfiguration,
  parseFlaggoServiceEndpoints,
} from "@flaggo/sdk/configuration";
import { createContractClient } from "@flaggo/sdk/management";

export async function deployContracts({
  manifestPath,
  services,
  credential,
  fetch,
  signal,
}) {
  const absoluteManifestPath = resolve(manifestPath);
  const manifestDirectory = dirname(absoluteManifestPath);
  const manifest = parseFlaggoDeploymentManifest(
    await readJson(absoluteManifestPath, signal),
  );
  const serviceUrls = parseFlaggoServiceEndpoints(services);

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
    baseUrl: serviceUrls.contractServiceUrl,
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

  const bindings = Object.fromEntries(contracts.map(({ deployment }) => [
    deployment.name,
    { contractDigest: deployment.contractDigest },
  ]));
  const runtimeConfig = parseFlaggoRuntimeConfiguration({
    format: "flaggo.runtime-config/v1",
    authority: manifest.authority,
    services: serviceUrls,
    bindings,
  });

  return {
    manifest,
    contracts,
    runtimeConfig,
  };
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
