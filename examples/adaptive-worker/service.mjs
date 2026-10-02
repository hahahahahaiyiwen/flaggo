import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { deployContracts } from "../contract-deployment.mjs";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
  sqliteDatabaseUrl,
  startContractAndDecisionHosts,
  startHost,
  startRustHost,
  waitForReady,
} from "../tetris-integration/host-process.mjs";
import { waitForServiceShutdown } from "./service-health.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const deploymentManifestPath = resolve(exampleDirectory, "flaggo.deploy.json");

export async function startAdaptiveWorkerService({
  lifecycle,
  runDirectory,
  writeRuntimeConfig = true,
}) {
  await mkdir(runDirectory, { recursive: true });
  lifecycle.signal.throwIfAborted();
  const paths = {
    database: resolve(runDirectory, "flaggo.db"),
    contractLog: resolve(runDirectory, "contract-service.log"),
    decisionLog: resolve(runDirectory, "decision-service.log"),
    otelIngestionLog: resolve(runDirectory, "otel-ingestion.log"),
    runtimeConfig: resolve(runDirectory, "flaggo.runtime.json"),
  };
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal === undefined
        ? lifecycle.signal
        : AbortSignal.any([init.signal, lifecycle.signal]),
    });
  const commonConfiguration = {
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
    Flaggo__Authentication__Application: "adaptive-worker",
    Flaggo__Authentication__Environment: "development",
  };
  const hosts = await startContractAndDecisionHosts({
    lifecycle,
    createContractHost: () => startHost(
      "adaptive-worker-contract-service",
      resolve(
        repositoryRoot,
        "apps/contract-service/src/Flaggo.ContractService/bin/Debug/net10.0/Flaggo.ContractService.dll",
      ),
      paths.contractLog,
      repositoryRoot,
      commonConfiguration,
    ),
    waitForContractReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
    createDecisionHost: () => startHost(
      "adaptive-worker-decision-service",
      resolve(
        repositoryRoot,
        "apps/decision-service/src/Flaggo.DecisionService/bin/Debug/net10.0/Flaggo.DecisionService.dll",
      ),
      paths.decisionLog,
      repositoryRoot,
      commonConfiguration,
    ),
    waitForDecisionReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
  });
  const otelIngestion = lifecycle.startHost(() => startRustHost(
    "adaptive-worker-otel-ingestion",
    "flaggo-otel-ingestion",
    paths.otelIngestionLog,
    repositoryRoot,
    {
      FLAGGO_DATABASE_URL: sqliteDatabaseUrl(paths.database),
    },
  ));
  const otelIngestionUrl = await otelIngestion.waitForListening({
    signal: lifecycle.signal,
  });
  lifecycle.assertHealthy();
  await waitForReady(
    (probeSignal) => ready(otelIngestionUrl, fetchWithAbort, probeSignal),
    otelIngestion,
    lifecycle.signal,
  );
  lifecycle.assertHealthy();
  const deployedContracts = await deployContracts({
    manifestPath: deploymentManifestPath,
    services: {
      contractServiceUrl: hosts.contractUrl,
      decisionServiceUrl: hosts.decisionUrl,
      otlpIngestionUrl: otelIngestionUrl,
    },
    credential: { mode: "local-development" },
    fetch: fetchWithAbort,
    signal: lifecycle.signal,
  });
  const deployed = deployedContracts.contracts[0];
  if (deployed === undefined || deployedContracts.contracts.length !== 1) {
    throw new Error(
      "The Adaptive Worker deployment manifest must contain exactly one DecisionContract.",
    );
  }
  const deployment = deployed.deployment;
  const runtimeConfig = deployedContracts.runtimeConfig;
  if (writeRuntimeConfig) {
    await writeFile(
      paths.runtimeConfig,
      `${JSON.stringify(runtimeConfig, null, 2)}\n`,
      "utf8",
    );
  }
  return {
    ...hosts,
    otelIngestion,
    otelIngestionUrl,
    runtimeConfig,
    contract: deployed.contract,
    deployment,
    paths,
  };
}

async function ready(url, fetchImpl, signal) {
  const response = await fetchImpl(`${url}/health/ready`, { signal });
  const body = await response.text();
  if (!response.ok) {
    throw new Error(`service readiness returned HTTP ${response.status}: ${body}`);
  }
  const readiness = JSON.parse(body);
  if (readiness.status !== "ready") {
    throw new Error(`service readiness reported '${readiness.status}': ${body}`);
  }
  return true;
}

async function runService(lifecycle, runDirectory) {
  const service = await startAdaptiveWorkerService({
    lifecycle,
    runDirectory,
  });
  process.stdout.write(`${JSON.stringify({
    status: "ready",
    contractServiceUrl: service.contractUrl,
    decisionServiceUrl: service.decisionUrl,
    otelIngestionUrl: service.otelIngestionUrl,
    contractDigest: service.deployment.contractDigest,
    runtimeConfigPath: service.paths.runtimeConfig,
  }, null, 2)}\n`);
  await waitForServiceShutdown(lifecycle);
}

async function main() {
  const runDirectory = resolve(repositoryRoot, ".flaggo", "adaptive-worker");
  await mkdir(dirname(runDirectory), { recursive: true });
  await mkdir(runDirectory);
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () => rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => runService(lifecycle, runDirectory),
      lifecycle,
    );
  } catch (error) {
    if (lifecycle.signalExitCode === undefined) {
      process.exitCode = 1;
      process.stderr.write(`${formatError(error)}\n`);
    }
  } finally {
    uninstallSignalHandlers();
  }
}

function formatError(error) {
  return error instanceof Error
    ? error.stack ?? error.message
    : String(error);
}

if (
  process.argv[1] !== undefined
  && pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
