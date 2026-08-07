import { createHash, randomUUID } from "node:crypto";
import {
  mkdir,
  open,
  readFile,
  rename,
  rm,
  stat,
} from "node:fs/promises";
import { basename, dirname, extname, join, resolve } from "node:path";

const atomicFileOperations = { mkdir, open, readFile, rename, rm, stat };
const artifactDescriptorFormat = "flaggo.committed-artifact";
const generationManifestFormat = "flaggo.committed-generation";
const safeArtifactNamePattern =
  /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u;

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
  await createDirectoryDurable(directory, signal, operations);
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
  let pointerRenamed = false;
  try {
    await createDirectoryDurable(generationPath, signal, operations);
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
    await syncDirectory(generationsPath, operations);
    signal?.throwIfAborted();
    await syncDirectory(absoluteRoot, operations);
    signal?.throwIfAborted();

    const manifest = {
      format: generationManifestFormat,
      version: 1,
      generation,
      files: Object.fromEntries(
        entries.map(([name], index) => [name, writes[index].value]),
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

export async function publishJsonArtifact(
  commitDescriptorPath,
  value,
  signal,
  operations = atomicFileOperations,
  artifactStem,
) {
  const descriptorPath = resolve(commitDescriptorPath);
  const directory = dirname(descriptorPath);
  const descriptorName = artifactStem ?? basename(
    descriptorPath,
    extname(descriptorPath),
  ).replace(/\.commit$/u, "");
  const artifact = `${descriptorName}-${randomUUID().replaceAll("-", "")}.json`;
  if (!isSafeArtifactName(artifact)) {
    throw new TypeError(
      "Commit descriptor generates an unsafe artifact filename.",
    );
  }

  signal?.throwIfAborted();
  await createDirectoryDurable(directory, signal, operations);
  const artifactPath = join(directory, artifact);
  let descriptorPublished = false;
  try {
    const entry = await writeJsonDurable(
      artifactPath,
      value,
      signal,
      operations,
    );
    signal?.throwIfAborted();
    await syncDirectory(directory, operations);
    signal?.throwIfAborted();
    try {
      await writeJsonAtomic(
        descriptorPath,
        {
          format: artifactDescriptorFormat,
          version: 1,
          ...entry,
        },
        signal,
        operations,
      );
      descriptorPublished = true;
    } catch (error) {
      descriptorPublished = error?.atomicRenameCompleted === true;
      throw error;
    }
    return {
      commitDescriptorPath: descriptorPath,
      artifactPath,
      ...entry,
    };
  } finally {
    if (!descriptorPublished) {
      await operations.rm(artifactPath, { force: true }).catch(() => {});
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
    manifest?.format !== generationManifestFormat
    || manifest?.version !== 1
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
  const artifacts = {};
  for (const name of requiredNames) {
    const entry = manifest.files[name];
    if (
      entry === null
      || typeof entry !== "object"
      || Array.isArray(entry)
      || !isSafeArtifactName(entry.artifact)
      || entry.artifact !== `${name}.json`
      || !Number.isSafeInteger(entry.byteLength)
      || entry.byteLength < 0
      || typeof entry.sha256 !== "string"
      || !/^[0-9a-f]{64}$/u.test(entry.sha256)
    ) {
      throw new Error(
        `Bootstrap generation manifest is missing '${name}'.`,
      );
    }
    const artifactPath = join(generationPath, entry.artifact);
    const bytes = await operations.readFile(artifactPath);
    const digest = createHash("sha256").update(bytes).digest("hex");
    if (
      bytes.byteLength !== entry.byteLength
      || digest !== entry.sha256
    ) {
      throw new Error(
        `Bootstrap generation artifact '${name}' does not match its manifest.`,
      );
    }
    paths[name] = artifactPath;
    artifacts[name] = entry;
  }
  return {
    rootPath: absoluteRoot,
    generation: manifest.generation,
    generationPath,
    manifestPath: join(absoluteRoot, "current.json"),
    paths,
    artifacts,
  };
}

async function writeJsonDurable(
  filePath,
  value,
  signal,
  operations,
) {
  const artifact = basename(filePath);
  const bytes = Buffer.from(
    `${JSON.stringify(value, null, 2)}\n`,
    "utf8",
  );
  let handle;
  try {
    handle = await operations.open(filePath, "wx");
    await handle.writeFile(bytes, { signal });
    signal?.throwIfAborted();
    await handle.sync();
  } finally {
    if (handle !== undefined) await handle.close();
  }
  return {
    artifact,
    byteLength: bytes.byteLength,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

export async function syncDirectory(directory, operations = atomicFileOperations) {
  let directoryHandle;
  try {
    directoryHandle = await operations.open(directory, "r");
  } catch (error) {
    if (!isUnsupportedDirectoryOpen(error)) throw error;
    return;
  }
  try {
    await directoryHandle.sync();
  } catch (error) {
    if (!isUnsupportedDirectoryFlush(error)) throw error;
  } finally {
    if (directoryHandle !== undefined) {
      await directoryHandle.close();
    }
  }
}

export async function createDirectoryDurable(
  directory,
  signal,
  operations = atomicFileOperations,
) {
  const absolutePath = resolve(directory);
  const missing = [];
  let cursor = absolutePath;
  while (!(await isDirectory(cursor, operations))) {
    const parent = dirname(cursor);
    if (parent === cursor) {
      throw new Error(
        `No existing ancestor was found for '${absolutePath}'.`,
      );
    }
    missing.push(cursor);
    cursor = parent;
  }

  if (missing.length > 0) {
    await syncDirectoryBoundary(cursor, operations);
  }

  let durableParent = cursor;
  for (const path of missing.reverse()) {
    signal?.throwIfAborted();
    try {
      await operations.mkdir(path, { recursive: false });
    } catch (error) {
      if (
        error?.code !== "EEXIST"
        || !(await isDirectory(path, operations))
      ) {
        throw error;
      }
    }
    await syncDirectory(durableParent, operations);
    await syncDirectory(path, operations);
    durableParent = path;
  }

  await syncDirectoryBoundary(absolutePath, operations);
}

async function syncDirectoryBoundary(directory, operations) {
  const immediateParent = dirname(directory);
  if (immediateParent !== directory) {
    await syncDirectory(immediateParent, operations);
  }
  await syncDirectory(directory, operations);
}

async function isDirectory(path, operations) {
  try {
    return (await operations.stat(path)).isDirectory();
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function isUnsupportedDirectoryOpen(error) {
  if (
    error?.code === "ENOTSUP"
    || error?.code === "EOPNOTSUPP"
  ) {
    return true;
  }
  return process.platform === "win32"
    && error?.code === "EISDIR";
}

function isUnsupportedDirectoryFlush(error) {
  if (
    error?.code === "ENOTSUP"
    || error?.code === "EOPNOTSUPP"
  ) {
    return true;
  }
  return process.platform === "win32"
    && ["EBADF", "EINVAL", "EPERM"].includes(error?.code);
}

function isSafeArtifactName(artifactName) {
  return typeof artifactName === "string"
    && safeArtifactNamePattern.test(artifactName);
}
