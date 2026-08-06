import { randomUUID } from "node:crypto";
import {
  mkdir,
  open,
  readFile,
  rename,
  rm,
} from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

const atomicFileOperations = { mkdir, open, readFile, rename, rm };

export async function writeJsonAtomic(
  filePath,
  value,
  signal,
  operations = atomicFileOperations,
) {
  const absolutePath = resolve(filePath);
  const directory = dirname(absolutePath);
  const stagingPath = `${absolutePath}.${process.pid}.${randomUUID()}.tmp`;
  let stagingHandle;
  signal?.throwIfAborted();
  await operations.mkdir(directory, { recursive: true });
  try {
    stagingHandle = await operations.open(stagingPath, "wx");
    await stagingHandle.writeFile(
      `${JSON.stringify(value, null, 2)}\n`,
      { encoding: "utf8", signal },
    );
    signal?.throwIfAborted();
    await stagingHandle.sync();
    await stagingHandle.close();
    stagingHandle = undefined;
    signal?.throwIfAborted();
    await operations.rename(stagingPath, absolutePath);
    try {
      await syncDirectory(directory, operations);
    } catch (error) {
      error.atomicRenameCompleted = true;
      throw error;
    }
  } finally {
    if (stagingHandle !== undefined) {
      await stagingHandle.close().catch(() => {});
    }
    await operations.rm(stagingPath, { force: true });
  }
}

export async function publishJsonGeneration(
  rootPath,
  files,
  signal,
  operations = atomicFileOperations,
) {
  const absoluteRoot = resolve(rootPath);
  const generationsPath = join(absoluteRoot, "generations");
  const generation = `${Date.now()}-${process.pid}-${randomUUID()}`;
  const generationPath = join(generationsPath, generation);
  const entries = Object.entries(files).sort(([left], [right]) =>
    left.localeCompare(right, "en")
  );
  if (
    entries.length === 0
    || entries.some(([name]) => !/^[a-z][a-z0-9-]*$/u.test(name))
  ) {
    throw new TypeError("Generation files require safe non-empty logical names.");
  }

  signal?.throwIfAborted();
  await operations.mkdir(generationsPath, { recursive: true });
  let pointerRenamed = false;
  try {
    await operations.mkdir(generationPath, { recursive: false });
    const writes = await Promise.allSettled(
      entries.map(([name, value]) =>
        writeJsonDurable(
          join(generationPath, `${name}.json`),
          value,
          signal,
          operations,
        )
      ),
    );
    const failures = writes
      .filter(({ status }) => status === "rejected")
      .map(({ reason }) => reason);
    if (failures.length > 0) {
      throw new AggregateError(
        failures,
        "Failed to write bootstrap generation files.",
      );
    }
    signal?.throwIfAborted();
    await syncDirectory(generationPath, operations);
    signal?.throwIfAborted();

    const manifest = {
      version: 1,
      generation,
      files: Object.fromEntries(
        entries.map(([name]) => [name, `${name}.json`]),
      ),
    };
    try {
      await writeJsonAtomic(
        join(absoluteRoot, "current.json"),
        manifest,
        signal,
        operations,
      );
      pointerRenamed = true;
    } catch (error) {
      pointerRenamed = error?.atomicRenameCompleted === true;
      throw error;
    }

    return {
      rootPath: absoluteRoot,
      generation,
      generationPath,
      manifestPath: join(absoluteRoot, "current.json"),
      paths: Object.fromEntries(
        entries.map(([name]) => [name, join(generationPath, `${name}.json`)]),
      ),
    };
  } finally {
    if (!pointerRenamed) {
      await operations.rm(generationPath, {
        recursive: true,
        force: true,
      }).catch(() => {});
    }
  }
}

export async function resolveJsonGeneration(
  rootPath,
  requiredNames,
  operations = atomicFileOperations,
) {
  const absoluteRoot = resolve(rootPath);
  const manifest = JSON.parse(
    await operations.readFile(
      join(absoluteRoot, "current.json"),
      { encoding: "utf8" },
    ),
  );
  if (
    manifest?.version !== 1
    || typeof manifest.generation !== "string"
    || !/^[0-9]+-[0-9]+-[0-9a-f-]+$/u.test(manifest.generation)
    || manifest.files === null
    || typeof manifest.files !== "object"
    || Array.isArray(manifest.files)
  ) {
    throw new Error("Bootstrap generation manifest is invalid.");
  }
  const generationPath = join(
    absoluteRoot,
    "generations",
    manifest.generation,
  );
  const paths = {};
  for (const name of requiredNames) {
    const fileName = manifest.files[name];
    if (
      typeof fileName !== "string"
      || fileName !== `${name}.json`
      || !/^[a-z][a-z0-9-]*\.json$/u.test(fileName)
    ) {
      throw new Error(
        `Bootstrap generation manifest is missing '${name}'.`,
      );
    }
    paths[name] = join(generationPath, fileName);
  }
  return {
    rootPath: absoluteRoot,
    generation: manifest.generation,
    generationPath,
    manifestPath: join(absoluteRoot, "current.json"),
    paths,
  };
}

async function writeJsonDurable(
  filePath,
  value,
  signal,
  operations,
) {
  let handle;
  try {
    handle = await operations.open(filePath, "wx");
    await handle.writeFile(
      `${JSON.stringify(value, null, 2)}\n`,
      { encoding: "utf8", signal },
    );
    signal?.throwIfAborted();
    await handle.sync();
  } finally {
    if (handle !== undefined) await handle.close();
  }
}

export async function syncDirectory(directory, operations = atomicFileOperations) {
  let directoryHandle;
  try {
    directoryHandle = await operations.open(directory, "r");
    await directoryHandle.sync();
  } catch (error) {
    if (!isUnsupportedDirectorySync(error)) throw error;
  } finally {
    if (directoryHandle !== undefined) {
      await directoryHandle.close();
    }
  }
}

function isUnsupportedDirectorySync(error) {
  if (
    error?.code === "ENOTSUP"
    || error?.code === "EOPNOTSUPP"
  ) {
    return true;
  }
  return process.platform === "win32"
    && ["EACCES", "EBADF", "EINVAL", "EISDIR", "EPERM"].includes(error?.code);
}
