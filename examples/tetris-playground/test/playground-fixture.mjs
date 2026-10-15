import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { basename, dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const exampleDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(exampleDirectory, "../..");

export async function readAttemptFailureRecords(runDirectory) {
  const entries = await readdir(runDirectory, { withFileTypes: true });
  if (!entries.some((entry) => entry.name === "analysis-workspaces" && entry.isDirectory())) {
    return { workspaceAvailable: false, failures: [] };
  }
  const workspaceRoot = resolve(runDirectory, "analysis-workspaces");
  const failures = [];
  for (const entry of await readdir(workspaceRoot, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith("-failure.json")) continue;
    const path = resolve(entry.parentPath, entry.name);
    const parts = relative(workspaceRoot, path).split(sep);
    if (parts.length !== 5 || parts[1] !== "cycles" || parts[3] !== "attempts") continue;
    failures.push({
      path: relative(runDirectory, path),
      record: JSON.parse(await readFile(path, "utf8")),
    });
  }
  failures.sort((left, right) => left.path.localeCompare(right.path));
  return { workspaceAvailable: true, failures };
}

export async function startPlaygroundFixture({ mode, analysisMode, retain = false }) {
  await mkdir(resolve(repositoryRoot, ".flaggo"), { recursive: true });
  const shutdownFile = resolve(repositoryRoot, ".flaggo", `playground-${randomUUID()}.stop`);
  let output = "";
  let retainedDirectory;
  const child = spawn(process.execPath, [
    resolve(exampleDirectory, "start.mjs"),
    `--mode=${mode}`,
    `--analysis=${analysisMode}`,
    ...(retain ? ["--retain"] : []),
  ], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      FLAGGO_PLAYGROUND_SHUTDOWN_FILE: shutdownFile,
    },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const append = (chunk) => {
    output = `${output}${String(chunk)}`.slice(-65_536);
    retainedDirectory ??= output.match(/^Run data will be retained at (.+)\.\r?$/mu)?.[1];
  };
  child.stdout.on("data", append);
  child.stderr.on("data", append);
  const fixture = {
    child,
    get output() { return output; },
    get runDirectory() {
      if (retainedDirectory === undefined) throw new Error("The retained run directory was not reported.");
      const directory = resolve(retainedDirectory);
      if (dirname(directory) !== resolve(repositoryRoot, ".flaggo")
        || !basename(directory).startsWith(`tetris-playground-${child.pid}-`)) {
        throw new Error("The reported run directory does not belong to this fixture.");
      }
      return directory;
    },
    async close() {
      try {
        if (child.exitCode === null) {
          const exit = once(child, "close");
          await writeFile(shutdownFile, "stop", "utf8");
          let deadline;
          try {
            const [code, signal] = await Promise.race([
              exit,
              new Promise((_, reject) => {
                deadline = setTimeout(() => reject(new Error("Playground cleanup timed out.")), 60_000);
              }),
            ]);
            if (code !== 0 || signal !== null) {
              throw new Error(`Playground exited with ${code ?? signal}.\n${output}`);
            }
          } finally {
            clearTimeout(deadline);
          }
        }
      } finally {
        await rm(shutdownFile, { force: true });
      }
    },
  };
  try {
    const url = await new Promise((resolvePromise, reject) => {
      const cleanup = () => {
        clearTimeout(deadline);
        clearInterval(interval);
        child.off("close", onClose);
      };
      const onClose = (code, signal) => {
        cleanup();
        reject(new Error(`Playground failed to start (${code ?? signal}).\n${output}`));
      };
      const deadline = setTimeout(() => {
        cleanup();
        reject(new Error(`Playground startup timed out.\n${output}`));
      }, 600_000);
      const interval = setInterval(() => {
        const match = output.match(/Flaggo Tetris playground: (http:\/\/127\.0\.0\.1:\d+)/u);
        if (match !== null) {
          cleanup();
          resolvePromise(match[1]);
        }
      }, 100);
      child.once("close", onClose);
    });
    return Object.assign(fixture, { url });
  } catch (error) {
    await fixture.close();
    if (retainedDirectory !== undefined) {
      await rm(fixture.runDirectory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    }
    throw error;
  }
}
