import { spawn } from "node:child_process";
import { once } from "node:events";
import { access, mkdir, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { deployContracts } from "../contract-deployment.mjs";
import { parsePlaygroundArguments, validatePlaygroundCredentials } from "./launch-options.mjs";
import {
  createHostLifecycle,
  formatIntegrationError,
  installSignalHandlers,
  runWithCleanup,
  startManagedProcess,
  waitForReady,
} from "../tetris-integration/host-process.mjs";
import {
  reserveLoopbackPort,
  startFlaggoServices,
  startObservabilityStack,
} from "./local-stack.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const options = parsePlaygroundArguments(process.argv.slice(2));
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `tetris-playground-${process.pid}-${Date.now()}`,
);

async function run(lifecycle) {
  validatePlaygroundCredentials(options);
  await mkdir(runDirectory, { recursive: true });
  if (options.retain) {
    process.stdout.write(`Run data will be retained at ${runDirectory}.\n`);
  }
  await runCommand("docker", ["info", "--format", "{{.ServerVersion}}"]);
  await buildPlayground();
  lifecycle.signal.throwIfAborted();

  const ingestionReservation = await reserveLoopbackPort();
  let ingestionReleased = false;
  try {
    const observability = await startObservabilityStack({
      exampleDirectory,
      ingestionPort: ingestionReservation.port,
      lifecycle,
      retain: options.retain,
      runDirectory,
    });
    await ingestionReservation.release();
    ingestionReleased = true;

    const services = await startFlaggoServices({
      analysisMode: options.analysisMode,
      exampleDirectory,
      ingestionPort: ingestionReservation.port,
      internalOtlpUrl: observability.endpoints.internalOtlpUrl,
      lifecycle,
      repositoryRoot,
      runDirectory,
    });
    const deployment = await deployContracts({
      manifestPath: resolve(exampleDirectory,
        options.mode === "live" ? "flaggo.live.deploy.json" : "flaggo.deploy.json"),
      services: {
        contractServiceUrl: services.contractUrl,
        decisionServiceUrl: services.decisionUrl,
        otlpIngestionUrl: services.otelIngestionUrl,
      },
      fetch: services.fetch,
      signal: lifecycle.signal,
    });
    const deployed = deployment.contracts[0];
    if (deployed === undefined || deployment.contracts.length !== 1) {
      throw new Error(
        "The playground manifest must deploy exactly one DecisionContract.",
      );
    }
    process.stdout.write(
      `Deployed ${deployed.deployment.name} `
      + `(${deployed.deployment.contractDigest}).\n`,
    );

    const materializer = await services.startEvidenceMaterializer();
    const analysis = await services.startAsyncAnalysis();
    const gateway = await startGateway({
      analysisMode: options.analysisMode,
      mode: options.mode,
      lifecycle,
      observability,
      runtimeConfig: {
        ...deployment.runtimeConfig,
        services: {
          ...deployment.runtimeConfig.services,
          otlpIngestionUrl: observability.endpoints.applicationOtlpUrl,
        },
      },
      services,
      materializer,
      analysis,
    });
    process.stdout.write(
      `\nFlaggo Tetris playground: ${gateway.url}\n`
      + `Grafana: ${observability.endpoints.grafanaUrl}\n`
      + `Analysis mode: ${options.analysisMode}\n`
      + `Playground mode: ${options.mode}\n`
      + `${options.retain
        ? "Press Ctrl+C to stop this run.\n"
        : "Press Ctrl+C to stop and remove this run.\n"}`,
    );

    await waitForShutdown(lifecycle);
  } finally {
    if (!ingestionReleased) await ingestionReservation.release();
  }
}

async function startGateway({
  analysisMode,
  mode,
  lifecycle,
  observability,
  runtimeConfig,
  services,
  materializer,
  analysis,
}) {
  const reservation = await reserveLoopbackPort();
  const address = `127.0.0.1:${reservation.port}`;
  const url = `http://${address}`;
  await reservation.release();

  const processIds = {
    "flaggo-contract-service": requiredPid(services.contract),
    "flaggo-decision-service": requiredPid(services.decision),
    "flaggo-otel-ingestion": requiredPid(services.otelIngestion),
    "flaggo-evidence-materializer": requiredPid(materializer),
    "flaggo-async-analysis": requiredPid(analysis),
  };
  const healthUrls = {
    "flaggo-contract-service": `${services.contractUrl}/health/ready`,
    "flaggo-decision-service": `${services.decisionUrl}/health/ready`,
    "flaggo-otel-ingestion": `${services.otelIngestionUrl}/health/ready`,
  };
  const host = lifecycle.startHost(() => {
    const processHost = startManagedProcess(
      "tetris-playground-gateway",
      process.execPath,
      [resolve(exampleDirectory, "dist/server.js")],
      resolve(runDirectory, "playground-gateway.log"),
      repositoryRoot,
      {
        ...process.env,
        FLAGGO_PLAYGROUND_ADDRESS: address,
        FLAGGO_PLAYGROUND_MODE: mode,
        FLAGGO_PLAYGROUND_DEMO_TIMEOUT_MS:
          analysisMode === "copilot" ? "600000" : "45000",
        FLAGGO_PLAYGROUND_GRAFANA_URL: observability.endpoints.grafanaUrl,
        FLAGGO_PLAYGROUND_HEALTH_URLS: JSON.stringify(healthUrls),
        FLAGGO_PLAYGROUND_LOKI_URL: observability.endpoints.lokiUrl,
        FLAGGO_PLAYGROUND_PROMETHEUS_URL:
          observability.endpoints.prometheusUrl,
        FLAGGO_PLAYGROUND_RUNTIME_CONFIG: JSON.stringify(runtimeConfig),
        FLAGGO_PLAYGROUND_SERVICE_PROCESSES: JSON.stringify(processIds),
        FLAGGO_PLAYGROUND_TEMPO_URL: observability.endpoints.tempoUrl,
      },
      url,
    );
    return gracefulGatewayHost(processHost, url);
  });
  await waitForReady(
    async (signal) => {
      const response = await fetch(`${url}/api/health`, { signal });
      if (!response.ok) {
        throw new Error(`playground health returned HTTP ${response.status}`);
      }
      return true;
    },
    host,
    lifecycle.signal,
    { timeoutMilliseconds: 30_000, retryDelayMilliseconds: 100 },
  );
  lifecycle.assertHealthy();
  return { host, url };
}

function gracefulGatewayHost(processHost, url) {
  let stopping = false;
  let stopPromise;
  return {
    name: processHost.name,
    get pid() {
      return processHost.pid;
    },
    get exited() {
      return processHost.exited;
    },
    get error() {
      return processHost.error;
    },
    get unexpectedExit() {
      return stopping ? undefined : processHost.unexpectedExit;
    },
    async stop() {
      stopPromise ??= (async () => {
        stopping = true;
        if (!processHost.exited) {
          try {
            await fetch(`${url}/api/shutdown`, {
              method: "POST",
              signal: AbortSignal.timeout(2_000),
            });
          } catch {
            // The managed process fallback below still guarantees termination.
          }
          const deadline = Date.now() + 10_000;
          while (!processHost.exited && Date.now() < deadline) {
            await new Promise((resolvePromise) =>
              setTimeout(resolvePromise, 50)
            );
          }
        }
        await processHost.stop();
      })();
      await stopPromise;
    },
  };
}

async function buildPlayground() {
  process.stdout.write("Building the local playground...\n");
  await runCommand("npm", [
    "run",
    "build:tetris-playground",
    "--workspaces=false",
  ]);
  await runCommand("cargo", [
    "build",
    "--locked",
    "-p",
    "flaggo-otel-ingestion",
    "-p",
    "flaggo-evidence-materializer",
    "-p",
    options.analysisMode === "deterministic"
      ? "flaggo-tetris-playground-analysis"
      : "flaggo-async-analysis",
  ]);
  await runCommand("dotnet", [
    "build",
    "Flaggo.slnx",
    "-c",
    "Debug",
    "--nologo",
  ]);
}

async function runCommand(executable, arguments_) {
  let command = executable;
  let commandArguments = arguments_;
  let shell = false;
  if (
    process.platform === "win32"
    && executable === "npm"
    && process.env.npm_execpath !== undefined
  ) {
    command = process.execPath;
    commandArguments = [process.env.npm_execpath, ...arguments_];
  } else if (process.platform === "win32" && executable === "npm") {
    shell = true;
  }
  const child = spawn(command, commandArguments, {
    cwd: repositoryRoot,
    env: process.env,
    shell,
    stdio: "inherit",
    windowsHide: true,
  });
  const [code, signal] = await once(child, "close");
  if (code !== 0) {
    throw new Error(
      `${executable} ${arguments_.join(" ")} failed `
      + `(code ${code ?? "unknown"}, signal ${signal ?? "none"}).`,
    );
  }
}

async function waitForShutdown(lifecycle) {
  while (!lifecycle.signal.aborted) {
    if (await shutdownRequested()) return;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 500));
    lifecycle.assertHealthy();
  }
  lifecycle.signal.throwIfAborted();
}

async function shutdownRequested() {
  const path = process.env.FLAGGO_PLAYGROUND_SHUTDOWN_FILE;
  if (path === undefined || path === "") return false;
  try {
    await access(path);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw new Error(`Unable to inspect the playground shutdown file.`, {
      cause: error,
    });
  }
}

function requiredPid(host) {
  const pid = host.pid;
  if (!Number.isInteger(pid) || pid <= 0) {
    throw new Error(`Host '${host.name}' has no process ID.`);
  }
  return pid;
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: options.retain
      ? async () => {}
      : () => rm(runDirectory, { recursive: true, force: true }),
    removeAttempts: 5,
    removeRetryDelayMilliseconds: 100,
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => run(lifecycle),
      lifecycle,
      (cleanupError) => {
        process.stderr.write(
          `Cleanup failure: ${formatIntegrationError(cleanupError)}\n`,
        );
      },
    );
    if (lifecycle.signalExitCode !== undefined) {
      process.exitCode = lifecycle.signalExitCode;
    }
  } catch (error) {
    if (lifecycle.signalExitCode !== undefined) {
      process.exitCode = lifecycle.signalExitCode;
    } else {
      process.exitCode = 1;
      process.stderr.write(`${formatIntegrationError(error)}\n`);
    }
  } finally {
    uninstallSignalHandlers();
  }
}

await main();
