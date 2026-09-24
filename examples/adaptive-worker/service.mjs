import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  createHostLifecycle,
  installSignalHandlers,
  runWithCleanup,
  startControlAndDataHosts,
  startHost,
  waitForReady,
} from "../tetris-integration/host-process.mjs";
import {
  bootstrapAdaptiveWorker,
  loadManifestBundle,
} from "./scenario.mjs";
import { waitForServiceShutdown } from "./service-health.mjs";

const exampleDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(exampleDirectory, "../..");

export async function startAdaptiveWorkerService({
  lifecycle,
  runDirectory,
  writeConnection = true,
}) {
  await mkdir(runDirectory, { recursive: true });
  lifecycle.signal.throwIfAborted();
  const paths = {
    registry: resolve(runDirectory, "definition-registry-v1.json"),
    bootstrap: resolve(runDirectory, "bootstrap"),
    audit: resolve(runDirectory, "audit.jsonl"),
    telemetry: resolve(runDirectory, "telemetry.jsonl"),
    connection: resolve(runDirectory, "service.json"),
    controlLog: resolve(runDirectory, "control-plane.log"),
    dataLog: resolve(runDirectory, "data-plane.log"),
  };
  const bundle = await loadManifestBundle(lifecycle.signal);
  const fetchWithAbort = (input, init = {}) =>
    fetch(input, {
      ...init,
      signal: init.signal ?? lifecycle.signal,
    });
  const commonConfiguration = {
    Flaggo__Authentication__LocalDevelopmentAppId:
      bundle.application.id,
    Flaggo__Authentication__LocalDevelopmentEnvironment:
      bundle.application.environment,
    Flaggo__Registry__LocalFilePath: paths.registry,
  };
  const runtime = await startControlAndDataHosts({
    lifecycle,
    createControlHost: () => startHost(
      "adaptive-worker-control-plane",
      resolve(
        repositoryRoot,
        "apps/control-plane/src/Flaggo.ControlPlane/bin/Debug/net10.0/Flaggo.ControlPlane.dll",
      ),
      paths.controlLog,
      repositoryRoot,
      commonConfiguration,
    ),
    waitForControlReady: (controlUrl, control, signal) =>
      waitForReady(async (probeSignal) => {
        const response = await fetchWithAbort(
          `${controlUrl}/v1/definition-bundles:validate`,
          {
            method: "POST",
            headers: {
              Authorization: "Flaggo-Local-Development",
              "Content-Type": "application/json",
            },
            body: JSON.stringify(bundle),
            signal: probeSignal,
          },
        );
        const body = await response.text();
        if (!response.ok) {
          throw new Error(
            `control-plane readiness returned HTTP ${response.status}: ${body}`,
          );
        }
        return true;
      }, control, signal),
    bootstrap: (controlUrl, signal) => bootstrapAdaptiveWorker({
      controlPlaneUrl: controlUrl,
      publicationPath: paths.bootstrap,
      fetchImpl: fetchWithAbort,
      signal,
    }),
    createDataHost: () => startHost(
      "adaptive-worker-data-plane",
      resolve(
        repositoryRoot,
        "apps/data-plane/src/Flaggo.DataPlane/bin/Debug/net10.0/Flaggo.DataPlane.dll",
      ),
      paths.dataLog,
      repositoryRoot,
      {
        ...commonConfiguration,
        Flaggo__Bootstrap__LocalGenerationPath: paths.bootstrap,
        Flaggo__Audit__LocalFilePath: paths.audit,
        Flaggo__Telemetry__CommitDescriptorPath:
          resolve(runDirectory, "telemetry", "current.commit.json"),
        "Flaggo__Targeting__AuthoritativeCohorts__worker-canary":
          "adaptive-workers",
        "Flaggo__Targeting__AuthoritativeCohorts__adaptive-workers":
          "adaptive-workers",
      },
    ),
    waitForDataReady: (dataUrl, data, signal) =>
      waitForReady(async (probeSignal) => {
        const response = await fetchWithAbort(
          `${dataUrl}/health/ready`,
          { signal: probeSignal },
        );
        const body = await response.text();
        if (!response.ok) {
          throw new Error(
            `data-plane readiness returned HTTP ${response.status}: ${body}`,
          );
        }
        const readiness = JSON.parse(body);
        if (readiness.status !== "ready") {
          throw new Error(
            `data-plane readiness reported '${readiness.status}': ${body}`,
          );
        }
        return true;
      }, data, signal),
  });
  const connection = {
    controlPlaneUrl: runtime.controlUrl,
    dataPlaneUrl: runtime.dataUrl,
    telemetryPath: paths.telemetry,
    receipt: runtime.bootstrap.receipt,
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
    bundle,
    connection,
    paths,
  };
}

async function runService(lifecycle, runDirectory) {
  const service = await startAdaptiveWorkerService({
    lifecycle,
    runDirectory,
  });
  process.stdout.write(`${JSON.stringify({
    status: "ready",
    controlPlaneUrl: service.controlUrl,
    dataPlaneUrl: service.dataUrl,
    connectionPath: service.paths.connection,
    telemetryPath: service.paths.telemetry,
    bundleDigest: service.bootstrap.receipt.bundleDigest,
  }, null, 2)}\n`);
  await waitForServiceShutdown(lifecycle);
}

async function main() {
  const runDirectory = resolve(
    repositoryRoot,
    ".flaggo",
    "adaptive-worker",
  );
  await mkdir(dirname(runDirectory), { recursive: true });
  await mkdir(runDirectory);
  const lifecycle = createHostLifecycle({
    removeRunDirectory: () =>
      rm(runDirectory, { recursive: true, force: true }),
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
  process.argv[1] !== undefined &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
