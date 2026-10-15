import { spawn } from "node:child_process";
import { once } from "node:events";
import { createWriteStream } from "node:fs";
import { mkdir } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { resolve } from "node:path";
import { analysisInferenceEnvironment } from "../tetris-integration/analysis-inference.mjs";

import {
  sqliteDatabaseUrl,
  startContractAndDecisionHosts,
  startHost,
  startManagedProcess,
  startRustHost,
  waitForReady,
} from "../tetris-integration/host-process.mjs";

const composeFileName = "compose.yaml";

export async function reserveLoopbackPort() {
  const server = createNetServer();
  server.unref();
  server.listen({ host: "127.0.0.1", port: 0, exclusive: true });
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string" || address.port <= 0) {
    server.close();
    throw new Error("Unable to reserve a loopback port.");
  }
  let released = false;
  return {
    port: address.port,
    async release() {
      if (released) return;
      released = true;
      await new Promise((resolvePromise, reject) => {
        server.close((error) => {
          if (error === undefined) resolvePromise();
          else reject(error);
        });
      });
    },
  };
}

export async function startObservabilityStack({
  exampleDirectory,
  ingestionPort,
  lifecycle,
  retain,
  runDirectory,
}) {
  const names = [
    "applicationOtlp",
    "internalOtlp",
    "collectorHealth",
    "grafana",
    "loki",
    "tempo",
    "prometheus",
  ];
  const reservations = await Promise.all(
    names.map(() => reserveLoopbackPort()),
  );
  const ports = Object.fromEntries(
    names.map((name, index) => [name, reservations[index].port]),
  );
  const dataDirectory = resolve(runDirectory, "lgtm-data");
  await mkdir(dataDirectory, { recursive: true });
  const projectName = `flaggo-playground-${process.pid}-${Date.now()}`
    .toLowerCase();
  const composeEnvironment = {
    ...process.env,
    FLAGGO_OTLP_INGESTION_ENDPOINT:
      `http://host.docker.internal:${ingestionPort}`,
    FLAGGO_PLAYGROUND_APPLICATION_OTLP_PORT: String(ports.applicationOtlp),
    FLAGGO_PLAYGROUND_COLLECTOR_HEALTH_PORT: String(ports.collectorHealth),
    FLAGGO_PLAYGROUND_GRAFANA_PORT: String(ports.grafana),
    FLAGGO_PLAYGROUND_INTERNAL_OTLP_PORT: String(ports.internalOtlp),
    FLAGGO_PLAYGROUND_LGTM_DATA: dataDirectory.replaceAll("\\", "/"),
    FLAGGO_PLAYGROUND_LOKI_PORT: String(ports.loki),
    FLAGGO_PLAYGROUND_PROMETHEUS_PORT: String(ports.prometheus),
    FLAGGO_PLAYGROUND_TEMPO_PORT: String(ports.tempo),
  };
  await Promise.all(reservations.map((reservation) => reservation.release()));

  const composeArguments = [
    "compose",
    "--ansi",
    "never",
    "--project-name",
    projectName,
    "--file",
    resolve(exampleDirectory, composeFileName),
  ];
  const processHost = startManagedProcess(
    "tetris-playground-observability",
    "docker",
    [...composeArguments, "up", "--remove-orphans"],
    resolve(runDirectory, "observability.log"),
    exampleDirectory,
    composeEnvironment,
    null,
  );
  const host = lifecycle.trackHost(wrapComposeHost({
    composeArguments,
    composeEnvironment,
    exampleDirectory,
    processHost,
    retain,
    runDirectory,
  }));

  const endpoints = {
    applicationOtlpUrl: `http://127.0.0.1:${ports.applicationOtlp}`,
    collectorHealthUrl: `http://127.0.0.1:${ports.collectorHealth}`,
    grafanaUrl: `http://127.0.0.1:${ports.grafana}`,
    internalOtlpUrl: `http://127.0.0.1:${ports.internalOtlp}`,
    lokiUrl: `http://127.0.0.1:${ports.loki}`,
    prometheusUrl: `http://127.0.0.1:${ports.prometheus}`,
    tempoUrl: `http://127.0.0.1:${ports.tempo}`,
  };
  await Promise.all([
    waitForHttp(`${endpoints.collectorHealthUrl}/`, host, lifecycle.signal),
    waitForHttp(`${endpoints.grafanaUrl}/api/health`, host, lifecycle.signal),
    waitForHttp(`${endpoints.lokiUrl}/ready`, host, lifecycle.signal),
    waitForHttp(`${endpoints.tempoUrl}/ready`, host, lifecycle.signal),
    waitForHttp(
      `${endpoints.prometheusUrl}/api/v1/status/runtimeinfo`,
      host,
      lifecycle.signal,
    ),
  ]);
  lifecycle.assertHealthy();
  return { endpoints, host, projectName };
}

export async function startFlaggoServices({
  analysisMode,
  exampleDirectory,
  ingestionPort,
  internalOtlpUrl,
  lifecycle,
  repositoryRoot,
  runDirectory,
}) {
  const paths = {
    analysisLog: resolve(runDirectory, "async-analysis.log"),
    analysisWorkspace: resolve(runDirectory, "analysis-workspaces"),
    contractLog: resolve(runDirectory, "contract-service.log"),
    copilotHome: resolve(runDirectory, "copilot-home"),
    database: resolve(runDirectory, "flaggo.db"),
    decisionLog: resolve(runDirectory, "decision-service.log"),
    materializerLog: resolve(runDirectory, "evidence-materializer.log"),
    otelIngestionLog: resolve(runDirectory, "otel-ingestion.log"),
  };
  await mkdir(runDirectory, { recursive: true });
  const databaseUrl = sqliteDatabaseUrl(paths.database);
  const telemetryEnvironment = {
    OTEL_BSP_SCHEDULE_DELAY: "100",
    OTEL_EXPORTER_OTLP_ENDPOINT: internalOtlpUrl,
    OTEL_EXPORTER_OTLP_PROTOCOL: "http/protobuf",
    OTEL_METRIC_EXPORT_INTERVAL: "500",
  };
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal === undefined
        ? lifecycle.signal
        : AbortSignal.any([init.signal, lifecycle.signal]),
    });

  const otelIngestion = lifecycle.startHost(() => startRustHost(
    "tetris-playground-otel-ingestion",
    "flaggo-otel-ingestion",
    paths.otelIngestionLog,
    repositoryRoot,
    {
      ...telemetryEnvironment,
      FLAGGO_DATABASE_URL: databaseUrl,
      FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS: `127.0.0.1:${ingestionPort}`,
    },
  ));
  const otelIngestionUrl = await otelIngestion.waitForListening({
    signal: lifecycle.signal,
  });
  await waitForReady(
    (signal) => ready(otelIngestionUrl, fetchWithAbort, signal),
    otelIngestion,
    lifecycle.signal,
  );

  const commonConfiguration = {
    ...telemetryEnvironment,
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
  };
  const hosts = await startContractAndDecisionHosts({
    lifecycle,
    createContractHost: () => startHost(
      "tetris-playground-contract-service",
      resolve(
        repositoryRoot,
        "apps/contract-service/src/Flaggo.ContractService/bin/Debug/net10.0/Flaggo.ContractService.dll",
      ),
      paths.contractLog,
      repositoryRoot,
      {
        ...commonConfiguration,
        FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS: "250",
      },
    ),
    waitForContractReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
    createDecisionHost: () => startHost(
      "tetris-playground-decision-service",
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

  async function startEvidenceMaterializer() {
    const host = lifecycle.startHost(() => startRustHost(
      "tetris-playground-evidence-materializer",
      "flaggo-evidence-materializer",
      paths.materializerLog,
      repositoryRoot,
      {
        ...telemetryEnvironment,
        FLAGGO_CONTRACT_CATALOG_URL:
          `${hosts.contractUrl}/v3/decision-contract-catalog/current`,
        FLAGGO_DATABASE_URL: databaseUrl,
        FLAGGO_MATERIALIZER_CATALOG_INTERVAL_SECONDS: "1",
        FLAGGO_MATERIALIZER_POLL_INTERVAL_MS: "25",
      },
      { reportsListeningUrl: false },
    ));
    await host.waitForStructuredLog(
      (entry) => entry["event.name"] === "flaggo.service.started",
      { signal: lifecycle.signal },
    );
    lifecycle.assertHealthy();
    return host;
  }

  async function startAsyncAnalysis() {
    const deterministic = analysisMode === "deterministic";
    const binaryName = deterministic
      ? "flaggo-tetris-playground-analysis"
      : "flaggo-async-analysis";
    const inferenceEnvironment = deterministic ? {} : analysisInferenceEnvironment();
    const host = lifecycle.startHost(() => startRustHost(
      "tetris-playground-async-analysis",
      binaryName,
      paths.analysisLog,
      repositoryRoot,
      {
        ...telemetryEnvironment,
        FLAGGO_ANALYSIS_COPILOT_HOME: paths.copilotHome,
        ...inferenceEnvironment,
        FLAGGO_ANALYSIS_MAX_AGENTS: "1",
        FLAGGO_ANALYSIS_POLL_INTERVAL_MS: "250",
        FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS: deterministic ? "120" : "600",
        FLAGGO_ANALYSIS_WORKSPACE_ROOT: paths.analysisWorkspace,
        FLAGGO_ANALYSIS_YIELD_GRACE_SECONDS: "2",
        FLAGGO_CONTRACT_CATALOG_URL:
          `${hosts.contractUrl}/v3/decision-contract-catalog/current`,
        FLAGGO_CONTRACT_SERVICE_URL: hosts.contractUrl,
        FLAGGO_DATABASE_URL: databaseUrl,
      },
      { reportsListeningUrl: false },
    ));
    await host.waitForStructuredLog(
      (entry) => entry["event.name"] === "flaggo.service.started",
      {
        signal: lifecycle.signal,
        timeoutMilliseconds: deterministic ? 30_000 : 120_000,
      },
    );
    lifecycle.assertHealthy();
    return host;
  }

  return {
    ...hosts,
    analysisMode,
    databaseUrl,
    fetch: fetchWithAbort,
    otelIngestion,
    otelIngestionUrl,
    paths,
    startAsyncAnalysis,
    startEvidenceMaterializer,
  };
}

function wrapComposeHost({
  composeArguments,
  composeEnvironment,
  exampleDirectory,
  processHost,
  retain,
  runDirectory,
}) {
  let stopPromise;
  return {
    name: processHost.name,
    get error() {
      return processHost.error;
    },
    get exited() {
      return processHost.exited;
    },
    get unexpectedExit() {
      return processHost.unexpectedExit;
    },
    async stop() {
      stopPromise ??= (async () => {
        const failures = [];
        try {
          await processHost.stop();
        } catch (error) {
          failures.push(error);
        }
        try {
          await runCommand(
            "docker",
            [
              ...composeArguments,
              "down",
              "--remove-orphans",
              ...(retain ? [] : ["--volumes"]),
            ],
            {
              cwd: exampleDirectory,
              env: composeEnvironment,
              logPath: resolve(runDirectory, "observability-cleanup.log"),
            },
          );
        } catch (error) {
          failures.push(error);
        }
        if (failures.length > 0) {
          throw new AggregateError(
            failures,
            "Failed to stop the playground observability stack.",
          );
        }
      })();
      await stopPromise;
    },
  };
}

async function waitForHttp(
  url,
  host,
  signal,
  timeoutMilliseconds = 120_000,
) {
  await waitForReady(
    async (probeSignal) => {
      const response = await fetch(url, { signal: probeSignal });
      if (!response.ok) {
        throw new Error(`${url} returned HTTP ${response.status}.`);
      }
      return true;
    },
    host,
    signal,
    { timeoutMilliseconds, retryDelayMilliseconds: 250 },
  );
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

async function runCommand(
  executable,
  arguments_,
  { cwd, env, logPath },
) {
  const log = createWriteStream(logPath, { flags: "a" });
  const child = spawn(executable, arguments_, {
    cwd,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.pipe(log, { end: false });
  child.stderr.pipe(log, { end: false });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr = `${stderr}${String(chunk)}`.slice(-8192);
  });
  const [code, signal] = await once(child, "close");
  log.end();
  await once(log, "close");
  if (code !== 0) {
    throw new Error(
      `${executable} ${arguments_.join(" ")} failed `
      + `(code ${code ?? "unknown"}, signal ${signal ?? "none"}).${
        stderr.trim() === "" ? "" : `\n${stderr.trim()}`
      }`,
    );
  }
}
