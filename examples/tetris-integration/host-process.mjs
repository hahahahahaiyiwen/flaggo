import { spawn } from "node:child_process";
import { once } from "node:events";
import { createWriteStream } from "node:fs";
import { createServer } from "node:net";

const dynamicLoopbackUrl = "http://127.0.0.1:0";
const signalExitCodes = new Map([
  ["SIGHUP", 129],
  ["SIGINT", 130],
  ["SIGTERM", 143],
]);

export class IntegrationSignalError extends Error {
  constructor(signal, exitCode) {
    super(`Tetris integration interrupted by ${signal}.`);
    this.name = "AbortError";
    this.signal = signal;
    this.exitCode = exitCode;
  }
}

export function createHostLifecycle({
  removeRunDirectory,
  removeRetryDelayMilliseconds = 25,
  removeAttempts = 3,
}) {
  if (typeof removeRunDirectory !== "function") {
    throw new TypeError("removeRunDirectory must be a function.");
  }
  if (!Number.isInteger(removeAttempts) || removeAttempts < 1) {
    throw new TypeError("removeAttempts must be a positive integer.");
  }

  const abortController = new AbortController();
  const hosts = [];
  const trackedHosts = new Set();
  const pendingFactories = new Set();
  const pendingStops = new Set();
  const cleanupFailures = [];
  const reportedUnexpectedExits = new WeakSet();
  let cleanupPromise;
  let cleanupStarted = false;
  let cleanupFinished = false;
  let signalExitCode;

  function unexpectedExitFailure(host) {
    if (host?.unexpectedExit === undefined) return undefined;
    return new Error(
      `Integration host '${host.name ?? "unnamed"}' exited before shutdown was requested.`,
      { cause: host.unexpectedExit },
    );
  }

  function collectUnexpectedExitFailure(host) {
    if (reportedUnexpectedExits.has(host)) return undefined;
    const failure = unexpectedExitFailure(host);
    if (failure !== undefined) reportedUnexpectedExits.add(host);
    return failure;
  }

  function assertHealthy() {
    const failures = hosts
      .map(unexpectedExitFailure)
      .filter((failure) => failure !== undefined);
    if (failures.length > 0) {
      throw new AggregateError(
        failures,
        "An integration host exited unexpectedly.",
      );
    }
  }

  function queueLateHostStop(host) {
    const settlement = Promise.resolve()
      .then(() => host.stop())
      .catch((error) => {
        cleanupFailures.push(
          new Error(
            `Failed to stop integration host '${host.name ?? "unnamed"}'.`,
            { cause: error },
          ),
        );
      })
      .finally(() => pendingStops.delete(settlement));
    pendingStops.add(settlement);
    return settlement;
  }

  function trackHost(host) {
    if (host === null || typeof host !== "object" || typeof host.stop !== "function") {
      throw new TypeError("A managed host must provide stop().");
    }
    if (cleanupStarted) {
      queueLateHostStop(host);
      throw new Error("Cannot track a host after integration cleanup began.");
    }
    if (!trackedHosts.has(host)) {
      trackedHosts.add(host);
      hosts.push(host);
    }
    assertHealthy();
    return host;
  }

  function startHostFactory(factory) {
    abortController.signal.throwIfAborted();
    if (cleanupStarted) {
      throw new Error("Cannot start a host after integration cleanup began.");
    }
    const host = factory();
    trackHost(host);
    abortController.signal.throwIfAborted();
    return host;
  }

  function startHostFactoryAsync(factory) {
    abortController.signal.throwIfAborted();
    if (cleanupStarted) {
      throw new Error("Cannot start a host after integration cleanup began.");
    }

    const entry = {
      cancelledByCleanup: false,
      error: undefined,
      host: undefined,
      settlement: undefined,
    };
    const factoryPromise = Promise.resolve().then(factory);
    entry.settlement = factoryPromise.then(
      async (host) => {
        try {
          if (cleanupStarted) {
            entry.host = host;
            entry.cancelledByCleanup = true;
            await queueLateHostStop(host);
            entry.error = new Error(
              "Integration host creation completed after cleanup began.",
            );
          } else {
            entry.host = trackHost(host);
          }
        } catch (error) {
          entry.error = error;
        }
      },
      (error) => {
        entry.error = error;
      },
    ).finally(() => {
      pendingFactories.delete(entry);
    });
    pendingFactories.add(entry);

    return entry.settlement.then(() => {
      if (entry.error !== undefined) throw entry.error;
      abortController.signal.throwIfAborted();
      return entry.host;
    });
  }

  function abortForSignal(signal) {
    const exitCode = signalExitCodes.get(signal);
    if (exitCode === undefined) {
      throw new Error(`Unsupported integration signal '${signal}'.`);
    }
    if (signalExitCode === undefined) {
      signalExitCode = exitCode;
      abortController.abort(new IntegrationSignalError(signal, exitCode));
    }
    return signalExitCode;
  }

  async function cleanup() {
    cleanupPromise ??= (async () => {
      cleanupStarted = true;
      while (true) {
        if (pendingFactories.size > 0) {
          const factories = [...pendingFactories];
          await Promise.all(factories.map((entry) => entry.settlement));
          for (const entry of factories) {
            pendingFactories.delete(entry);
            if (entry.error !== undefined && !entry.cancelledByCleanup) {
              cleanupFailures.push(
                new Error("Failed to create an integration host.", {
                  cause: entry.error,
                }),
              );
            }
          }
        }

        for (const host of hosts) {
          const failure = collectUnexpectedExitFailure(host);
          if (failure !== undefined) cleanupFailures.push(failure);
        }

        while (hosts.length > 0) {
          const batch = hosts.splice(0).reverse();
          const results = await Promise.allSettled(
            batch.map(async (host) => {
              await host.stop();
            }),
          );
          for (let index = 0; index < results.length; index += 1) {
            const result = results[index];
            if (result.status === "rejected") {
              const host = batch[index];
              cleanupFailures.push(
                new Error(
                  `Failed to stop integration host '${host.name ?? "unnamed"}'.`,
                  { cause: result.reason },
                ),
              );
            }
          }
        }

        if (pendingStops.size > 0) {
          await Promise.all([...pendingStops]);
        }

        if (
          hosts.length === 0
          && pendingFactories.size === 0
          && pendingStops.size === 0
        ) break;
      }

      let removalError;
      for (let attempt = 1; attempt <= removeAttempts; attempt += 1) {
        try {
          await removeRunDirectory();
          removalError = undefined;
          break;
        } catch (error) {
          removalError = error;
          if (attempt < removeAttempts && removeRetryDelayMilliseconds > 0) {
            await new Promise((resolvePromise) =>
              setTimeout(resolvePromise, removeRetryDelayMilliseconds)
            );
          }
        }
      }
      if (removalError !== undefined) {
        cleanupFailures.push(
          new Error("Failed to remove the integration run directory.", {
            cause: removalError,
          }),
        );
      }

      cleanupFinished = true;
      if (cleanupFailures.length > 0) {
        throw new AggregateError(
          cleanupFailures,
          "Tetris integration cleanup failed.",
        );
      }
    })();
    await cleanupPromise;
  }

  return {
    signal: abortController.signal,
    get signalExitCode() {
      return signalExitCode;
    },
    abortForSignal,
    trackHost,
    startHost: startHostFactory,
    startHostAsync: startHostFactoryAsync,
    assertHealthy,
    cleanup,
  };
}

export function installSignalHandlers(
  lifecycle,
  processTarget = process,
) {
  const handlers = new Map();
  for (const signal of signalExitCodes.keys()) {
    const handler = () => {
      processTarget.exitCode = lifecycle.abortForSignal(signal);
    };
    handlers.set(signal, handler);
    processTarget.on(signal, handler);
  }
  return () => {
    for (const [signal, handler] of handlers) {
      processTarget.off(signal, handler);
    }
  };
}

export async function runWithCleanup(
  workflow,
  lifecycle,
  reportCleanupFailure = () => {},
) {
  let result;
  let workflowFailure;
  try {
    result = await workflow(lifecycle.signal);
  } catch (error) {
    workflowFailure = error;
  }

  let cleanupFailure;
  try {
    await lifecycle.cleanup();
  } catch (error) {
    cleanupFailure = error;
  }

  if (workflowFailure !== undefined) {
    if (cleanupFailure !== undefined) {
      try {
        reportCleanupFailure(cleanupFailure, workflowFailure);
      } catch {
        // Reporting must never replace the workflow failure.
      }
    }
    throw workflowFailure;
  }
  if (cleanupFailure !== undefined) throw cleanupFailure;
  return result;
}

export async function startControlAndDataHosts({
  lifecycle,
  createControlHost,
  waitForControlReady,
  bootstrap,
  createDataHost,
  waitForDataReady,
}) {
  const control = lifecycle.startHost(createControlHost);
  const controlUrl = await control.waitForListening({
    signal: lifecycle.signal,
  });
  lifecycle.assertHealthy();
  await waitForControlReady(controlUrl, control, lifecycle.signal);
  lifecycle.assertHealthy();
  lifecycle.signal.throwIfAborted();

  const bootstrapResult = await bootstrap(controlUrl, lifecycle.signal);
  lifecycle.assertHealthy();
  lifecycle.signal.throwIfAborted();

  const data = lifecycle.startHost(() => createDataHost(bootstrapResult));
  const dataUrl = await data.waitForListening({
    signal: lifecycle.signal,
  });
  lifecycle.assertHealthy();
  await waitForDataReady(dataUrl, data, lifecycle.signal);
  lifecycle.assertHealthy();
  lifecycle.signal.throwIfAborted();

  return {
    bootstrap: bootstrapResult,
    control,
    controlUrl,
    data,
    dataUrl,
  };
}

export function parseListeningUrl(line) {
  let entry;
  try {
    entry = JSON.parse(line);
  } catch {
    return undefined;
  }
  const address = entry?.Category === "Microsoft.Hosting.Lifetime"
    ? entry?.State?.address
    : undefined;
  if (typeof address !== "string") return undefined;

  let url;
  try {
    url = new URL(address);
  } catch {
    return undefined;
  }
  const port = Number(url.port);
  if (
    url.protocol !== "http:"
    || url.hostname !== "127.0.0.1"
    || !Number.isInteger(port)
    || port <= 0
    || url.username !== ""
    || url.password !== ""
    || url.search !== ""
    || url.hash !== ""
    || url.pathname !== "/"
  ) {
    return undefined;
  }
  return url.origin;
}

export function createStructuredLogObserver(onListening) {
  let buffer = "";

  function processLine(line) {
    const url = parseListeningUrl(line.endsWith("\r") ? line.slice(0, -1) : line);
    if (url !== undefined) onListening(url);
  }

  return {
    write(chunk) {
      buffer += String(chunk);
      let lineEnd;
      while ((lineEnd = buffer.indexOf("\n")) >= 0) {
        processLine(buffer.slice(0, lineEnd));
        buffer = buffer.slice(lineEnd + 1);
      }
    },
    end() {
      if (buffer !== "") processLine(buffer);
      buffer = "";
    },
  };
}

export function startHost(
  name,
  assembly,
  logPath,
  repositoryRoot,
  configuration,
) {
  const log = createWriteStream(logPath, { flags: "a" });
  let logError;
  const logClosed = new Promise((resolvePromise) => {
    log.once("close", resolvePromise);
    log.once("error", (error) => {
      logError = error;
      resolvePromise();
    });
  });
  const child = spawn("dotnet", [assembly], {
    cwd: repositoryRoot,
    env: createHostEnvironment(configuration),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.pipe(log, { end: false });
  child.stderr.pipe(log, { end: false });

  let listeningSettled = false;
  let resolveListening;
  let rejectListening;
  const listeningUrl = new Promise((resolvePromise, reject) => {
    resolveListening = resolvePromise;
    rejectListening = reject;
  });
  const observer = createStructuredLogObserver((url) => {
    if (!listeningSettled) {
      listeningSettled = true;
      resolveListening(url);
    }
  });
  child.stdout.on("data", (chunk) => observer.write(chunk));
  child.stdout.once("end", () => observer.end());

  let exited = false;
  let closed = false;
  let spawnError;
  let exitCode;
  let exitSignal;
  let shutdownRequested = false;
  let unexpectedExit;
  const childClosed = new Promise((resolvePromise) => {
    child.once("error", (error) => {
      spawnError = error;
      exited = true;
      if (!shutdownRequested) unexpectedExit = error;
      if (!listeningSettled) {
        listeningSettled = true;
        rejectListening(error);
      }
    });
    child.once("exit", (code, signal) => {
      exited = true;
      exitCode = code;
      exitSignal = signal;
      if (!shutdownRequested) {
        unexpectedExit = new Error(
          `${name} exited unexpectedly `
          + `(code ${code ?? "unknown"}, signal ${signal ?? "none"}).`,
        );
      }
    });
    child.once("close", (code, signal) => {
      closed = true;
      exited = true;
      exitCode ??= code;
      exitSignal ??= signal;
      if (!listeningSettled) {
        listeningSettled = true;
        rejectListening(
          new Error(
            `${name} exited before reporting its listening URL `
            + `(code ${exitCode ?? "unknown"}, signal ${exitSignal ?? "none"}).`,
          ),
        );
      }
      log.end();
      resolvePromise();
    });
  });

  let stopPromise;
  return {
    name,
    get exited() {
      return exited;
    },
    get error() {
      return spawnError;
    },
    get unexpectedExit() {
      return unexpectedExit;
    },
    async waitForListening({
      timeoutMilliseconds = 30000,
      signal,
    } = {}) {
      let timer;
      let abortHandler;
      try {
        const candidates = [
          listeningUrl,
          new Promise((_, reject) => {
            timer = setTimeout(
              () => reject(
                new Error(
                  `${name} did not report a listening URL within `
                  + `${timeoutMilliseconds}ms.`,
                ),
              ),
              timeoutMilliseconds,
            );
          }),
        ];
        if (signal !== undefined) {
          signal.throwIfAborted();
          candidates.push(new Promise((_, reject) => {
            abortHandler = () => reject(signal.reason);
            signal.addEventListener("abort", abortHandler, { once: true });
          }));
        }
        return await Promise.race(candidates);
      } finally {
        clearTimeout(timer);
        if (abortHandler !== undefined) {
          signal.removeEventListener("abort", abortHandler);
        }
      }
    },
    async stop() {
      stopPromise ??= (async () => {
        shutdownRequested = true;
        if (!closed) {
          if (!exited) child.kill();
          if (!(await closesWithin(childClosed, 3000))) {
            if (!exited) child.kill("SIGKILL");
            child.stdout.destroy();
            child.stderr.destroy();
            if (!(await closesWithin(childClosed, 3000))) {
              throw new Error(`${name} did not exit after forced termination.`);
            }
          }
        }
        if (!log.writableEnded) log.end();
        await logClosed;
        if (logError !== undefined) throw logError;
      })();
      await stopPromise;
    },
  };
}

export function createHostEnvironment(
  configuration,
  parentEnvironment = process.env,
) {
  return {
    ...parentEnvironment,
    ...configuration,
    ASPNETCORE_ENVIRONMENT: "Development",
    DOTNET_ENVIRONMENT: "Development",
    ASPNETCORE_URLS: dynamicLoopbackUrl,
    Logging__Console__FormatterName: "json",
    Flaggo__Authentication__LocalDevelopmentBypass: "true",
  };
}

export async function waitForReady(
  probe,
  host,
  signal,
  {
    timeoutMilliseconds = 30000,
    retryDelayMilliseconds = 100,
  } = {},
) {
  const deadline = Date.now() + timeoutMilliseconds;
  let lastError;
  while (Date.now() < deadline) {
    signal.throwIfAborted();
    if (host.exited) {
      throw new Error(
        `${host.name} exited before becoming ready${host.error === undefined
          ? "."
          : `: ${host.error.message}`}`,
      );
    }

    const remaining = deadline - Date.now();
    const timeout = new AbortController();
    const timer = setTimeout(
      () => timeout.abort(
        new DOMException(
          `${host.name} readiness probe exceeded its remaining deadline.`,
          "TimeoutError",
        ),
      ),
      remaining,
    );
    try {
      const probeSignal = AbortSignal.any([signal, timeout.signal]);
      if (await probe(probeSignal)) return;
    } catch (error) {
      signal.throwIfAborted();
      lastError = error;
    } finally {
      clearTimeout(timer);
    }

    const delay = Math.min(retryDelayMilliseconds, deadline - Date.now());
    if (delay > 0) {
      await abortableDelay(delay, signal);
    }
  }
  throw new Error(
    `${host.name} did not become ready: ${lastError?.message ?? "timeout"}`,
  );
}

function abortableDelay(milliseconds, signal) {
  signal.throwIfAborted();
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolvePromise();
    }, milliseconds);
    function onAbort() {
      clearTimeout(timer);
      reject(signal.reason);
    }
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

export async function startUnavailableEndpoint() {
  const sockets = new Set();
  const server = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    socket.destroy();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string" || address.port <= 0) {
    server.close();
    throw new Error("Failed to bind the unavailable integration endpoint.");
  }

  let stopPromise;
  return {
    name: "unavailable-endpoint",
    url: `http://127.0.0.1:${address.port}`,
    get exited() {
      return !server.listening;
    },
    get error() {
      return undefined;
    },
    async stop() {
      stopPromise ??= (async () => {
        const socketClosures = [...sockets].map((socket) => once(socket, "close"));
        for (const socket of sockets) socket.destroy();
        await Promise.all([
          ...socketClosures,
          new Promise((resolvePromise, reject) => {
            server.close((error) => {
              if (error === undefined) {
                resolvePromise();
              } else {
                reject(error);
              }
            });
          }),
        ]);
      })();
      await stopPromise;
    },
  };
}

export async function startHungEndpoint() {
  const sockets = new Set();
  const server = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string" || address.port <= 0) {
    server.close();
    throw new Error("Failed to bind the hung integration endpoint.");
  }

  let stopPromise;
  return {
    name: "hung-endpoint",
    url: `http://127.0.0.1:${address.port}`,
    get exited() {
      return !server.listening;
    },
    get error() {
      return undefined;
    },
    get connectionCount() {
      return sockets.size;
    },
    async stop() {
      stopPromise ??= (async () => {
        const socketClosures = [...sockets].map((socket) => once(socket, "close"));
        for (const socket of sockets) socket.destroy();
        await Promise.all([
          ...socketClosures,
          new Promise((resolvePromise, reject) => {
            server.close((error) => {
              if (error === undefined) {
                resolvePromise();
              } else {
                reject(error);
              }
            });
          }),
        ]);
      })();
      await stopPromise;
    },
  };
}

async function closesWithin(childClosed, timeoutMilliseconds) {
  return Promise.race([
    childClosed.then(() => true),
    new Promise((resolvePromise) =>
      setTimeout(() => resolvePromise(false), timeoutMilliseconds)
    ),
  ]);
}
