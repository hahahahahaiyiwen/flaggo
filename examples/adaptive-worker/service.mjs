import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  createContractServiceClient,
} from "../../packages/sdk-typescript/dist/index.js";
import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
  startContractAndDecisionHosts,
  startHost,
  waitForReady,
} from "../tetris-integration/host-process.mjs";
import { waitForServiceShutdown } from "./service-health.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const contractPath = resolve(exampleDirectory, "decision-contract.json");

export async function startAdaptiveWorkerService({
  lifecycle,
  runDirectory,
  writeConnection = true,
}) {
  await mkdir(runDirectory, { recursive: true });
  lifecycle.signal.throwIfAborted();
  const paths = {
    database: resolve(runDirectory, "flaggo.db"),
    telemetry: resolve(runDirectory, "telemetry.jsonl"),
    connection: resolve(runDirectory, "service.json"),
    contractLog: resolve(runDirectory, "contract-service.log"),
    decisionLog: resolve(runDirectory, "decision-service.log"),
  };
  const contract = JSON.parse(
    await readFile(contractPath, { encoding: "utf8", signal: lifecycle.signal }),
  );
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal ?? lifecycle.signal,
    });
  const commonConfiguration = {
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
    Flaggo__Authentication__Application: "adaptive-worker",
    Flaggo__Authentication__Environment: "development",
  };
  const runtime = await startContractAndDecisionHosts({
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
    publishContract: async (url) => {
      const client = createContractServiceClient({
        contractServiceUrl: url,
        credential: { mode: "local-development" },
        fetch: fetchWithAbort,
      });
      return client.put(contract.name, contract);
    },
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
  const connection = {
    contractServiceUrl: runtime.contractUrl,
    decisionServiceUrl: runtime.decisionUrl,
    contracts: {
      [runtime.publication.name]: {
        contractDigest: runtime.publication.contractDigest,
      },
    },
    telemetryPath: paths.telemetry,
  };
  if (writeConnection) {
    await writeFile(
      paths.connection,
      `${JSON.stringify(connection, null, 2)}\n`,
      "utf8",
    );
  }
  return {
    ...runtime,
    connection,
    contract,
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
    contractDigest: service.publication.contractDigest,
    connectionPath: service.paths.connection,
    telemetryPath: service.paths.telemetry,
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
