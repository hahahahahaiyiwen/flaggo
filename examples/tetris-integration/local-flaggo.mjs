import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

import {
  startContractAndDecisionHosts,
  startHost,
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
  };
  const commonConfiguration = {
    ConnectionStrings__Flaggo: `Data Source=${paths.database};Pooling=False`,
    Flaggo__Authentication__Application: "tetris",
    Flaggo__Authentication__Environment: "integration",
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

  return {
    ...hosts,
    fetch: fetchWithAbort,
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
