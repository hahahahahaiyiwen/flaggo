import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

import {
  parseMaterializationHealth,
  sqliteDatabaseUrl,
  startContractAndDecisionHosts,
  startRustHost,
  waitForReady,
} from "./host-process.mjs";

export async function startLocalFlaggoHosts({
  lifecycle,
  repositoryRoot,
  exampleDirectory,
  runDirectory,
  analysisMode = "copilot",
}) {
  if (analysisMode !== "copilot" && analysisMode !== "deterministic") {
    throw new TypeError("analysisMode must be 'copilot' or 'deterministic'.");
  }
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
    createContractHost: () => startRustHost(
      "tetris-contract-service",
      "flaggo-contract-service",
      paths.contractLog,
      repositoryRoot,
      {
        FLAGGO_CONTRACT_DATABASE_PATH: paths.database,
        FLAGGO_CONTRACT_LISTEN_ADDRESS: "127.0.0.1:0",
        FLAGGO_CONTRACT_ACTIVATION_POLL_INTERVAL_MS: "250",
      },
    ),
    waitForContractReady: (url, host, signal) =>
      waitForReady(
        (probeSignal) => ready(url, fetchWithAbort, probeSignal),
        host,
        signal,
      ),
    createDecisionHost: () => startRustHost(
      "tetris-decision-service",
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
      (entry) => entry["event.name"] === "flaggo.service.started",
      { signal: lifecycle.signal },
    );
    lifecycle.assertHealthy();
    return {
      host,
      startup: {
        ...startup,
        activeRoutes: startup["flaggo.materializer.active_route_count"],
        currentContracts:
          startup["flaggo.materializer.current_contract_count"],
        evidenceStore: {
          conflictCount:
            startup["flaggo.materializer.evidence.conflict_count"],
          diagnosticCount:
            startup["flaggo.materializer.evidence.diagnostic_count"],
          hasCachedCatalog:
            startup["flaggo.materializer.evidence.has_cached_catalog"],
          observationCount:
            startup["flaggo.materializer.evidence.observation_count"],
          provenanceCount:
            startup["flaggo.materializer.evidence.provenance_count"],
        },
        materialization: parseMaterializationHealth(startup),
      },
    };
  }

  async function startAsyncAnalysis({ githubToken } = {}) {
    const deterministic = analysisMode === "deterministic";
    const inferenceEnvironment = deterministic ? {} : analysisInferenceEnvironment({
      ...process.env,
      ...(githubToken === undefined ? {} : { GH_TOKEN: githubToken }),
    });
    const host = lifecycle.startHost(() => startRustHost(
      "tetris-async-analysis",
      deterministic ? "flaggo-tetris-playground-analysis" : "flaggo-async-analysis",
      paths.asyncAnalysisLog,
      repositoryRoot,
      {
        FLAGGO_ANALYSIS_COPILOT_HOME: paths.copilotHome,
        ...inferenceEnvironment,
        FLAGGO_ANALYSIS_POLL_INTERVAL_MS: "1000",
        FLAGGO_ANALYSIS_RUN_TIMEOUT_SECONDS: deterministic ? "120" : "600",
        FLAGGO_ANALYSIS_WORKSPACE_ROOT: paths.asyncAnalysisWorkspace,
        FLAGGO_CONTRACT_CATALOG_URL:
          `${hosts.contractUrl}/v3/decision-contract-catalog/current`,
        FLAGGO_CONTRACT_SERVICE_URL: hosts.contractUrl,
        FLAGGO_DATABASE_URL: databaseUrl,
      },
      { reportsListeningUrl: false },
    ));
    const startup = await host.waitForStructuredLog(
      (entry) => entry["event.name"] === "flaggo.service.started",
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
import { analysisInferenceEnvironment } from "./analysis-inference.mjs";
