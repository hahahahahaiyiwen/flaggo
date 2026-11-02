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
  const hosts = await startContractAndDecisionHosts({
    lifecycle,
    createContractHost: () => startRustHost(
      "adaptive-worker-contract-service",
      "flaggo-contract-service",
      paths.contractLog,
      repositoryRoot,
      {
        FLAGGO_CONTRACT_DATABASE_PATH: paths.database,
        FLAGGO_CONTRACT_LISTEN_ADDRESS: "127.0.0.1:0",
      },
    ),
    waitForContractReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
    createDecisionHost: () => startRustHost(
      "adaptive-worker-decision-service",
      "flaggo-decision-service",
      paths.decisionLog,
      repositoryRoot,
      {
        FLAGGO_DECISION_DATABASE_PATH: paths.database,
        FLAGGO_DECISION_LISTEN_ADDRESS: "127.0.0.1:0",
      },
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
