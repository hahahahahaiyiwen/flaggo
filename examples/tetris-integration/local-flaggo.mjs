import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

import {
  parseMaterializationHealth,
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

  return {
    ...hosts,
    databaseUrl,
    fetch: fetchWithAbort,
    otelIngestion,
    otelIngestionUrl,
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
