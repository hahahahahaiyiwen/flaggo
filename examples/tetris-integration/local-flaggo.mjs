import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

import {
  sqliteDatabaseUrl,
  startContractAndDecisionHosts,
  startHost,
  startRustHost,
  waitForReady,
} from "./host-process.mjs";

export async function startLocalFlaggoHosts({
  lifecycle,
  repositoryRoot,
  exampleDirectory,
  runDirectory,
}) {
  await mkdir(runDirectory, { recursive: true });
  const paths = {
    database: resolve(runDirectory, "flaggo.db"),
    contractLog: resolve(runDirectory, "contract-service.log"),
    decisionLog: resolve(runDirectory, "decision-service.log"),
    asyncAnalysisLog: resolve(runDirectory, "async-analysis.log"),
    asyncAnalysisWorkspace: resolve(runDirectory, "w"),
    copilotHome: resolve(runDirectory, "c"),
    evidenceMaterializerLog: resolve(runDirectory, "evidence-materializer.log"),
    otelIngestionLog: resolve(runDirectory, "otel-ingestion.log"),
  };
  const commonConfiguration = {
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
  };
  const databaseUrl = sqliteDatabaseUrl(paths.database);
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal === undefined
        ? lifecycle.signal
        : AbortSignal.any([init.signal, lifecycle.signal]),
    });

  const hosts = await startContractAndDecisionHosts({
    lifecycle,
    createContractHost: () => startHost(
      "tetris-contract-service",
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
      "tetris-decision-service",
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
    "tetris-otel-ingestion",
    "flaggo-otel-ingestion",
    paths.otelIngestionLog,
    repositoryRoot,
    {
      FLAGGO_DATABASE_URL: databaseUrl,
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

  async function startEvidenceMaterializer() {
    const host = lifecycle.startHost(() => startRustHost(
      "tetris-evidence-materializer",
      "flaggo-evidence-materializer",
      paths.evidenceMaterializerLog,
      repositoryRoot,
      {
        FLAGGO_CONTRACT_CATALOG_URL:
          `${hosts.contractUrl}/v3/decision-contract-catalog/current`,
        FLAGGO_DATABASE_URL: databaseUrl,
        FLAGGO_MATERIALIZER_CATALOG_INTERVAL_SECONDS: "1",
        FLAGGO_MATERIALIZER_POLL_INTERVAL_MS: "25",
      },
      { reportsListeningUrl: false },
    ));
    const startup = await host.waitForStructuredLog(
      (entry) => entry.event === "materializer.started",
      { signal: lifecycle.signal },
    );
    lifecycle.assertHealthy();
    return { host, startup };
  }

  async function startAsyncAnalysis({ githubToken }) {
    if (typeof githubToken !== "string" || githubToken.trim() === "") {
      throw new TypeError("githubToken must be a non-empty string.");
    }
    const host = lifecycle.startHost(() => startRustHost(
      "tetris-async-analysis",
      "flaggo-async-analysis",
      paths.asyncAnalysisLog,
      repositoryRoot,
      {
        FLAGGO_ANALYSIS_COPILOT_HOME: paths.copilotHome,
        FLAGGO_ANALYSIS_GITHUB_TOKEN: githubToken,
        FLAGGO_ANALYSIS_LOG_SESSION_EVENTS: "true",
        FLAGGO_ANALYSIS_POLL_INTERVAL_MS: "1000",
        FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS: "300",
        FLAGGO_ANALYSIS_WORKSPACE_ROOT: paths.asyncAnalysisWorkspace,
        FLAGGO_CONTRACT_CATALOG_URL:
          `${hosts.contractUrl}/v3/decision-contract-catalog/current`,
        FLAGGO_CONTRACT_SERVICE_URL: hosts.contractUrl,
        FLAGGO_DATABASE_URL: databaseUrl,
      },
      { reportsListeningUrl: false },
    ));
    const startup = await host.waitForStructuredLog(
      (entry) => entry.event === "async_analysis.started",
      {
        signal: lifecycle.signal,
        timeoutMilliseconds: 120000,
      },
    );
    lifecycle.assertHealthy();
    return { host, startup };
  }

  return {
    ...hosts,
    databaseUrl,
    fetch: fetchWithAbort,
    otelIngestion,
    otelIngestionUrl,
    paths,
    startAsyncAnalysis,
    startEvidenceMaterializer,
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
