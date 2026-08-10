import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { createInterface } from "node:readline";
import {
  dirname,
  isAbsolute,
  relative,
  resolve,
} from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const safeLogicalName = /^[a-z][a-z0-9-]*$/u;
const defaultServerController = createWindowsDirectoryHelperServer();

export const windowsDirectoryHelper = createWindowsDirectoryHelper();

export async function flushWindowsDirectory(path) {
  return windowsDirectoryHelper.flush(path);
}

export async function validateWindowsPath(path) {
  return windowsDirectoryHelper.validate(path);
}

export async function writeWindowsAtomic(root, path, value) {
  return windowsDirectoryHelper.writeAtomic(root, path, value);
}

export async function ensureWindowsDirectory(root, path) {
  return windowsDirectoryHelper.ensureDirectory(root, path);
}

export async function publishWindowsArtifact(
  root,
  descriptorPath,
  value,
  artifactStem,
) {
  return windowsDirectoryHelper.publishArtifact(
    root,
    descriptorPath,
    value,
    artifactStem,
  );
}

export async function publishWindowsGeneration(root, files) {
  return windowsDirectoryHelper.publishGeneration(root, files);
}

export async function resolveWindowsGeneration(root, requiredNames) {
  return windowsDirectoryHelper.resolveGeneration(root, requiredNames);
}

export function createWindowsDirectoryHelper({
  platform = process.platform,
  invoke = invokeServer,
} = {}) {
  function requireWindows() {
    if (platform !== "win32") {
      throw new Error("The Windows directory helper requires win32.");
    }
  }

  return {
    async flush(path) {
      requireWindows();
      const absolutePath = absolute(path, "path");
      const result = await invoke({
        operation: "flush",
        path: absolutePath,
      });
      return requiredString(result?.resolvedPath, "resolvedPath");
    },

    async validate(path) {
      requireWindows();
      const absolutePath = absolute(path, "path");
      const result = await invoke({
        operation: "validate",
        path: absolutePath,
      });
      return requiredString(result?.resolvedPath, "resolvedPath");
    },

    async writeAtomic(root, path, value) {
      requireWindows();
      const absoluteRoot = absolute(root, "root");
      const absolutePath = absolute(path, "path");
      assertContained(absoluteRoot, absolutePath);
      return invoke({
        operation: "write-atomic",
        root: absoluteRoot,
        path: absolutePath,
        contentBase64: jsonBytes(value).toString("base64"),
      });
    },

    async ensureDirectory(root, path) {
      requireWindows();
      const absoluteRoot = absolute(root, "root");
      const absolutePath = absolute(path, "path");
      assertContained(absoluteRoot, absolutePath);
      return invoke({
        operation: "ensure-directory",
        root: absoluteRoot,
        path: absolutePath,
      });
    },

    async publishArtifact(root, descriptorPath, value, artifactStem) {
      requireWindows();
      const absoluteRoot = absolute(root, "root");
      const absolutePath = absolute(descriptorPath, "path");
      assertContained(absoluteRoot, absolutePath);
      if (
        artifactStem !== undefined
        && (
          typeof artifactStem !== "string"
          || artifactStem.length === 0
        )
      ) {
        throw new TypeError("artifactStem must be a non-empty string.");
      }
      return invoke({
        operation: "publish-artifact",
        root: absoluteRoot,
        path: absolutePath,
        contentBase64: jsonBytes(value).toString("base64"),
        ...(artifactStem === undefined ? {} : { artifactStem }),
      });
    },

    async publishGeneration(root, files) {
      requireWindows();
      const absoluteRoot = absolute(root, "root");
      const entries = Object.entries(files ?? {})
        .sort(([left], [right]) => left.localeCompare(right, "en"));
      if (
        entries.length === 0
        || entries.some(([name]) => !safeLogicalName.test(name))
      ) {
        throw new TypeError(
          "Generation files require safe non-empty logical names.",
        );
      }
      return invoke({
        operation: "publish-generation",
        root: absoluteRoot,
        files: Object.fromEntries(
          entries.map(([name, value]) => [
            name,
            jsonBytes(value).toString("base64"),
          ]),
        ),
      });
    },

    async resolveGeneration(root, requiredNames) {
      requireWindows();
      const absoluteRoot = absolute(root, "root");
      if (
        !Array.isArray(requiredNames)
        || requiredNames.length === 0
        || new Set(requiredNames).size !== requiredNames.length
        || requiredNames.some((name) =>
          typeof name !== "string" || !safeLogicalName.test(name)
        )
      ) {
        throw new TypeError("Required generation names are invalid.");
      }
      const result = await invoke({
        operation: "resolve-generation",
        root: absoluteRoot,
        requiredNames,
      });
      if (
        result === null
        || typeof result !== "object"
        || result.contents === null
        || typeof result.contents !== "object"
      ) {
        throw new Error("Windows directory helper returned an invalid result.");
      }
      for (const name of requiredNames) {
        const content = result.contents[name];
        if (typeof content !== "string") {
          throw new Error(
            `Windows directory helper omitted committed artifact '${name}'.`,
          );
        }
        JSON.parse(Buffer.from(content, "base64").toString("utf8"));
      }
      return result;
    },
  };
}

async function invokeServer(request) {
  return defaultServerController.invoke(request);
}

export function createWindowsDirectoryHelperServer({
  findExecutable = findHelper,
  spawnProcess = spawn,
  processTarget = process,
  environment = process.env,
  idleMilliseconds = 250,
  manageProcessLifecycle = true,
} = {}) {
  let activeServer;
  let initializationPromise;
  let startingChild;
  let idleTimer;
  let disposed = false;

  const exitHandler = () => stopSynchronously();
  const signalHandlers = new Map([
    ["SIGINT", () => handleSignal("SIGINT")],
    ["SIGTERM", () => handleSignal("SIGTERM")],
  ]);
  if (manageProcessLifecycle) {
    processTarget.once("exit", exitHandler);
    for (const [signal, handler] of signalHandlers) {
      processTarget.on(signal, handler);
    }
  }

  async function invoke(request) {
    if (disposed) {
      throw new Error("Windows directory helper server is shut down.");
    }
    clearTimeout(idleTimer);
    const active = await ensureServer();
    const id = randomUUID();
    return new Promise((resolvePromise, rejectPromise) => {
      let line;
      try {
        line = `${JSON.stringify({ ...request, id })}\n`;
      } catch (error) {
        rejectPromise(error);
        return;
      }
      active.pending.set(id, {
        resolve: resolvePromise,
        reject: rejectPromise,
      });
      try {
        active.child.stdin.write(line, (error) => {
          if (error !== null && error !== undefined) {
            active.pending.delete(id);
            rejectPromise(error);
          }
        });
      } catch (error) {
        active.pending.delete(id);
        rejectPromise(error);
      }
    }).finally(scheduleIdleShutdown);
  }

  async function ensureServer() {
    if (
      activeServer !== undefined
      && activeServer.child.exitCode === null
      && activeServer.child.signalCode === null
    ) {
      return activeServer;
    }
    if (initializationPromise !== undefined) {
      return initializationPromise;
    }

    const currentInitialization = startServer();
    initializationPromise = currentInitialization;
    try {
      return await currentInitialization;
    } finally {
      if (initializationPromise === currentInitialization) {
        initializationPromise = undefined;
      }
    }
  }

  async function startServer() {
    const executable = await findExecutable();
    if (disposed) {
      throw new Error("Windows directory helper server is shut down.");
    }
    const child = spawnProcess(
      executable.command,
      [...executable.args, "--server"],
      {
        cwd: repositoryRoot,
        env: environment,
        stdio: ["pipe", "pipe", "pipe"],
        windowsHide: true,
      },
    );
    startingChild = child;
    let current;

    try {
      const pending = new Map();
      const stderr = [];
      let finished = false;
      let terminalError;
      child.stdin.on("error", () => {});
      child.stderr.setEncoding("utf8");
      child.stderr.on("data", (chunk) => stderr.push(chunk));
      const reader = createInterface({ input: child.stdout });
      current = {
        child,
        pending,
        reader,
        stopping: undefined,
        closed: false,
      };
      child.once("close", () => {
        current.closed = true;
      });

      reader.on("line", (line) => {
        let response;
        try {
          response = JSON.parse(line);
        } catch (error) {
          failAll(
            pending,
            new Error(`Directory helper returned invalid JSON: ${line}`, {
              cause: error,
            }),
          );
          return;
        }
        const waiter = pending.get(response.id);
        if (waiter === undefined) return;
        pending.delete(response.id);
        if (response.ok === true) {
          waiter.resolve(response.result);
        } else {
          const helperError = new Error(
            response.error?.message ?? "Windows directory helper failed.",
          );
          helperError.code = response.error?.code ?? "HELPER_ERROR";
          waiter.reject(helperError);
        }
      });

      function finish(error) {
        if (finished) return;
        finished = true;
        terminalError = error;
        reader.close();
        if (activeServer?.child === child) activeServer = undefined;
        failAll(pending, error);
      }

      child.once("error", (error) => finish(error));
      child.once("exit", (code, signal) => {
        finish(new Error(
          `Windows directory helper exited with code ${code}` +
          `${signal === null ? "" : ` and signal ${signal}`}: ` +
          stderr.join("").trim(),
        ));
      });

      await waitForSpawn(child);
      if (finished) throw terminalError;
      if (disposed) {
        throw new Error("Windows directory helper server is shut down.");
      }
      activeServer = current;
      return current;
    } catch (error) {
      if (current === undefined) {
        await stopChild(child);
      } else {
        await stopServer(current);
      }
      throw error;
    } finally {
      if (startingChild === child) startingChild = undefined;
    }
  }

  function scheduleIdleShutdown() {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      if (activeServer !== undefined && activeServer.pending.size === 0) {
        const idleServer = activeServer;
        activeServer = undefined;
        void stopServer(idleServer);
      }
    }, idleMilliseconds);
    idleTimer.unref();
  }

  async function reset() {
    clearTimeout(idleTimer);
    const initializing = initializationPromise;
    if (initializing !== undefined) {
      await initializing.catch(() => {});
    }
    const current = activeServer;
    activeServer = undefined;
    if (current !== undefined) {
      await stopServer(current);
    }
  }

  async function shutdown() {
    if (disposed) return;
    disposed = true;
    if (manageProcessLifecycle) {
      for (const [signal, handler] of signalHandlers) {
        processTarget.removeListener(signal, handler);
      }
    }
    await reset();
    if (manageProcessLifecycle) {
      processTarget.removeListener("exit", exitHandler);
    }
  }

  async function handleSignal(signal) {
    try {
      await shutdown();
    } finally {
      try {
        processTarget.kill(processTarget.pid, signal);
      } catch {
        processTarget.exitCode = 1;
      }
    }
  }

  function stopSynchronously() {
    clearTimeout(idleTimer);
    const current = activeServer;
    activeServer = undefined;
    current?.reader.close();
    const children = new Set([
      current?.child,
      startingChild,
    ]);
    startingChild = undefined;
    for (const child of children) {
      if (child === undefined) continue;
      child.stdin?.end();
      if (
        child.pid !== undefined
        && child.exitCode === null
        && child.signalCode === null
      ) {
        child.kill();
      }
    }
  }

  return { invoke, reset, shutdown };
}

function failAll(pending, error) {
  for (const waiter of pending.values()) waiter.reject(error);
  pending.clear();
}

function waitForSpawn(child) {
  return new Promise((resolvePromise, rejectPromise) => {
    const cleanup = () => {
      child.removeListener("spawn", onSpawn);
      child.removeListener("error", onError);
      child.removeListener("exit", onExit);
    };
    const onSpawn = () => {
      cleanup();
      resolvePromise();
    };
    const onError = (error) => {
      cleanup();
      rejectPromise(error);
    };
    const onExit = (code, signal) => {
      cleanup();
      rejectPromise(new Error(
        `Windows directory helper exited during startup with code ${code}` +
        `${signal === null ? "" : ` and signal ${signal}`}.`,
      ));
    };
    child.once("spawn", onSpawn);
    child.once("error", onError);
    child.once("exit", onExit);
  });
}

async function stopServer(server) {
  if (server.stopping !== undefined) {
    return server.stopping;
  }
  server.stopping = (async () => {
    server.reader.close();
    if (!server.closed && (
      server.child.exitCode !== null
      || server.child.signalCode !== null
    )) {
      await waitForClose(server, 1_000);
      return;
    }

    if (!server.closed) server.child.stdin.end();
    if (await waitForClose(server, 1_000)) return;
    if (!server.closed &&
      server.child.pid !== undefined
      && server.child.exitCode === null
      && server.child.signalCode === null
    ) {
      server.child.kill();
    }
    await waitForClose(server, 1_000);
  })();
  return server.stopping;
}

async function stopChild(child) {
  child.stdin?.end();
  if (await waitForChildClose(child, 1_000)) return;
  if (
    child.pid !== undefined
    && child.exitCode === null
    && child.signalCode === null
  ) {
    child.kill();
  }
  await waitForChildClose(child, 1_000);
}

function waitForClose(server, timeoutMilliseconds) {
  if (server.closed) {
    return Promise.resolve(true);
  }
  return new Promise((resolvePromise) => {
    const timer = setTimeout(() => {
      server.child.removeListener("close", onClose);
      resolvePromise(false);
    }, timeoutMilliseconds);
    const onClose = () => {
      clearTimeout(timer);
      resolvePromise(true);
    };
    server.child.once("close", onClose);
  });
}

function waitForChildClose(child, timeoutMilliseconds) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve(true);
  }
  return new Promise((resolvePromise) => {
    const timer = setTimeout(() => {
      child.removeListener("close", onClose);
      resolvePromise(false);
    }, timeoutMilliseconds);
    const onClose = () => {
      clearTimeout(timer);
      resolvePromise(true);
    };
    child.once("close", onClose);
  });
}

export async function resetWindowsDirectoryHelperForTests() {
  await defaultServerController.reset();
}

function jsonBytes(value) {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function absolute(path, name) {
  if (typeof path !== "string" || path.trim().length === 0) {
    throw new TypeError(`${name} must be a non-empty path.`);
  }
  const absolutePath = resolve(path);
  if (!isAbsolute(absolutePath)) {
    throw new TypeError(`${name} must resolve to an absolute path.`);
  }
  return absolutePath;
}

function assertContained(root, path) {
  const rel = relative(root, path);
  if (
    rel === ".."
    || rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`)
    || isAbsolute(rel)
  ) {
    throw new Error(
      `Publication path '${path}' escapes configured root '${root}'.`,
    );
  }
}

function requiredString(value, name) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `Windows directory helper omitted result field '${name}'.`,
    );
  }
  return value;
}

async function findHelper() {
  const configured = process.env.FLAGGO_DIRECTORY_FLUSH_HELPER;
  const candidates = configured === undefined
    ? [
        resolve(
          repositoryRoot,
          "tools/Flaggo.DirectoryFlush/bin/Debug/net10.0/" +
          "Flaggo.DirectoryFlush.exe",
        ),
        resolve(
          repositoryRoot,
          "tools/Flaggo.DirectoryFlush/bin/Release/net10.0/" +
          "Flaggo.DirectoryFlush.exe",
        ),
      ]
    : [resolve(configured)];
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return { command: candidate, args: [] };
    } catch {
    }
  }
  throw new Error(
    "Windows durable directory helper is not built. Run " +
    "'dotnet build Flaggo.slnx' or set FLAGGO_DIRECTORY_FLUSH_HELPER.",
  );
}
