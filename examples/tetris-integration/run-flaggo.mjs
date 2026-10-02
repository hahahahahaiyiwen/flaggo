import { rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
} from "./host-process.mjs";
import { deployTetrisContract } from "./deploy-contract.mjs";
import { startLocalFlaggoHosts } from "./local-flaggo.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");
const runDirectory = resolve(
  repositoryRoot,
  ".flaggo",
  `tetris-play-${process.pid}-${Date.now()}`,
);

async function play(lifecycle) {
  process.stdout.write("Starting local Flaggo services...\n");
  const hosts = await startLocalFlaggoHosts({
    lifecycle,
    repositoryRoot,
    exampleDirectory,
    runDirectory,
  });
  lifecycle.assertHealthy();
  const deployed = await deployTetrisContract({
    exampleDirectory,
    contractUrl: hosts.contractUrl,
    fetch: hosts.fetch,
    signal: lifecycle.signal,
  });

  const [
    { createFlaggoDropIntervalProvider },
    { TetrisTelemetryProviders },
    { runTerminalTetris },
  ] = await Promise.all([
    import("./dist/flaggo/flaggo-provider.js"),
    import("./dist/flaggo/otel.js"),
    import("./dist/flaggo/terminal.js"),
  ]);
  const telemetry = new TetrisTelemetryProviders({
    environment: "integration",
    flaggoOtlpBaseUrl: hosts.otelIngestionUrl,
  });
  try {
    const provider = createFlaggoDropIntervalProvider({
      baseUrl: hosts.decisionUrl,
      contractDigest: deployed.deployment.contractDigest,
      credential: { mode: "local-development" },
      fetch: hosts.fetch,
      telemetry: telemetry.policyInstrumentation,
    });
    await runTerminalTetris({
      instrumentation: telemetry.sessionInstrumentation,
      provider,
      signal: lifecycle.signal,
    });
  } finally {
    try {
      await telemetry.forceFlush();
    } finally {
      await telemetry.shutdown();
    }
  }
}

async function main() {
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () =>
      rm(runDirectory, { recursive: true, force: true }),
  });
  const uninstallSignalHandlers = installSignalHandlers(lifecycle);
  try {
    await runWithCleanup(
      () => play(lifecycle),
      lifecycle,
      (cleanupError) => {
        process.stderr.write(`Cleanup failure: ${formatError(cleanupError)}\n`);
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

await main();
