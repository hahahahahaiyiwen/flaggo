import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deployContracts } from "../../examples/contract-deployment.mjs";
import { loadDeployedTarget } from "./deployed-target.mjs";
import { verifyDecisions } from "./verify-bound-decision.mjs";

const [releaseRoot, stateRoot, descriptorPath, gatewayEnvironment] = process.argv.slice(2);
if (![releaseRoot, stateRoot, descriptorPath, gatewayEnvironment].every((value) =>
  typeof value === "string" && resolve(value) === value)) {
  throw new Error("Specify absolute release, state, descriptor and gateway-environment paths.");
}
const identity = JSON.parse(await readFile(resolve(stateRoot, ".flaggo-state.json"), "utf8"));
if (identity.format !== "flaggo.state/v1" || identity.kind !== "disposable-smoke"
  || !identity.stateId || process.env.FLAGGO_DEPLOYED_SMOKE_CONFIRM !== identity.stateId) {
  throw new Error("Binding mutates state: confirm the exact disposable smoke state identity.");
}
const services = {
  contractServiceUrl: "http://127.0.0.1:8080",
  decisionServiceUrl: "http://127.0.0.1:8081",
  otlpIngestionUrl: "http://127.0.0.1:5090",
};
const deployment = await deployContracts({
  manifestPath: fileURLToPath(new URL("../../examples/tetris-playground/flaggo.deploy.json", import.meta.url)),
  services, signal: AbortSignal.timeout(30_000),
});
if (deployment.contracts.length !== 1) throw new Error("Smoke requires exactly one Tetris contract.");
const manifestBytes = await readFile(resolve(releaseRoot, "release.json"));
const manifest = JSON.parse(manifestBytes.toString("utf8"));
const descriptor = {
  format: "flaggo.deployed-smoke/v1", analysis: "reviewed-python-exit",
  disposable: true, stateId: identity.stateId, releaseRoot, stateRoot,
  sourceCommit: manifest.sourceCommit,
  releaseManifestSha256: createHash("sha256").update(manifestBytes).digest("hex"),
  contractDigest: deployment.contracts[0].deployment.contractDigest,
  gatewayUrl: "http://127.0.0.1:4310", services,
  observability: { lokiUrl: "http://127.0.0.1:3100", tempoUrl: "http://127.0.0.1:3200" },
};
await writeFile(descriptorPath, JSON.stringify(descriptor, null, 2));
const target = await loadDeployedTarget(descriptorPath, identity.stateId);
const parity = await verifyDecisions(
  target, deployment.manifest.authority, deployment.contracts[0].contract.result.default,
);
await writeFile(`${descriptorPath}.default-parity.json`, JSON.stringify(parity, null, 2));
const runtimeConfig = {
  ...deployment.runtimeConfig,
  services: { ...services, otlpIngestionUrl: "http://127.0.0.1:4318" },
};
const environment = await readFile(gatewayEnvironment, "utf8");
if (!/^FLAGGO_PLAYGROUND_RUNTIME_CONFIG=.+$/mu.test(environment)) {
  throw new Error("Gateway environment must contain an explicit runtime-config line.");
}
await writeFile(gatewayEnvironment, environment.replace(
  /^FLAGGO_PLAYGROUND_RUNTIME_CONFIG=.+$/mu,
  `FLAGGO_PLAYGROUND_RUNTIME_CONFIG=${JSON.stringify(runtimeConfig)}`,
));
