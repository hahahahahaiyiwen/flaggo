import { createHash, randomUUID } from "node:crypto";
import {
  constants,
  lstat,
  mkdir,
  open,
  readFile,
  realpath,
  rename,
  rm,
  stat,
} from "node:fs/promises";
import {
  basename,
  dirname,
  extname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import {
  ensureWindowsDirectory,
  flushWindowsDirectory,
  publishWindowsArtifact,
  publishWindowsGeneration,
  resolveWindowsGeneration,
  validateWindowsPath,
  writeWindowsAtomic,
} from "./windows-directory-helper.mjs";

const atomicFileOperations = {
  mkdir,
  open,
  readFile,
  rename,
  rm,
  stat,
  lstat,
  realpath,
  platform: process.platform,
  createFileFlags: process.platform === "win32"
    ? "wx"
    : constants.O_WRONLY
      | constants.O_CREAT
      | constants.O_EXCL
      | constants.O_NOFOLLOW,
  readFileFlags: process.platform === "win32"
    ? "r"
    : constants.O_RDONLY | constants.O_NOFOLLOW,
  flushDirectory: flushWindowsDirectory,
  validateWindowsPath,
  windowsNativeHelper: process.platform === "win32",
  linuxSecureTraversal: process.platform === "linux",
};
const artifactDescriptorFormat = "flaggo.committed-artifact";
const generationManifestFormat = "flaggo.committed-generation";
const safeArtifactNamePattern =
  /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/u;

export async function writeJsonAtomic(
  filePath,
  value,
  signal,
  operations = atomicFileOperations,
  allowedRoot = dirname(resolve(filePath)),
  rootIdentity,
) {
  const absolutePath = resolve(filePath);
  const directory = dirname(absolutePath);
  if (usesWindowsNativeHelper(operations)) {
    signal?.throwIfAborted();
    await writeWindowsAtomic(resolve(allowedRoot), absolutePath, value);
    signal?.throwIfAborted();
    return;
  }
  if (usesLinuxSecureTraversal(operations)) {
    return writeJsonAtomicLinux(
      absolutePath,
      value,
      signal,
      resolve(allowedRoot),
    );
  }
  const stagingPath = `${absolutePath}.${process.pid}.${randomUUID()}.tmp`;
  let stagingHandle;
  signal?.throwIfAborted();
  const ensuredRootIdentity = await createDirectoryDurable(
    directory,
    signal,
    operations,
    allowedRoot,
  );
  rootIdentity ??= ensuredRootIdentity;
  await assertDirectoryIdentity(allowedRoot, rootIdentity, operations);
  try {
    await assertSafePath(allowedRoot, stagingPath, operations, {
      allowMissing: true,
    });
    stagingHandle = await operations.open(
      stagingPath,
      operations.createFileFlags ?? "wx",
    );
    await assertOpenedPathSafe(
      allowedRoot,
      stagingPath,
      stagingHandle,
      operations,
      rootIdentity,
    );
    await stagingHandle.writeFile(
      `${JSON.stringify(value, null, 2)}\n`,
      { encoding: "utf8", signal },
    );
    signal?.throwIfAborted();
    await stagingHandle.sync();
    await stagingHandle.close();
    stagingHandle = undefined;
    signal?.throwIfAborted();
    await assertDirectoryIdentity(allowedRoot, rootIdentity, operations);
    await assertSafePath(allowedRoot, absolutePath, operations, {
      allowMissing: true,
    });
    await operations.rename(stagingPath, absolutePath);
    await assertSafePath(allowedRoot, absolutePath, operations);
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
  if (usesWindowsNativeHelper(operations)) {
    const result = await publishWindowsGeneration(absoluteRoot, files);
    signal?.throwIfAborted();
    return result;
  }
  if (usesLinuxSecureTraversal(operations)) {
    return publishJsonGenerationLinux(absoluteRoot, entries, signal);
  }
  let pointerRenamed = false;
  try {
    const rootIdentity = await createDirectoryDurable(
      generationPath,
      signal,
      operations,
      absoluteRoot,
    );
    const writes = await Promise.allSettled(
      entries.map(([name, value]) =>
        writeJsonDurable(
          join(generationPath, `${name}.json`),
          value,
          signal,
          operations,
          absoluteRoot,
          rootIdentity,
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
    await assertDirectoryIdentity(absoluteRoot, rootIdentity, operations);
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
        absoluteRoot,
        rootIdentity,
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
  if (usesWindowsNativeHelper(operations)) {
    const result = await publishWindowsArtifact(
      directory,
      descriptorPath,
      value,
      artifactStem,
    );
    signal?.throwIfAborted();
    return result;
  }
  if (usesLinuxSecureTraversal(operations)) {
    return publishJsonArtifactLinux(
      descriptorPath,
      value,
      signal,
      artifact,
    );
  }
  const rootIdentity = await createDirectoryDurable(
    directory,
    signal,
    operations,
    directory,
  );
  const artifactPath = join(directory, artifact);
  let descriptorPublished = false;
  try {
    const entry = await writeJsonDurable(
      artifactPath,
      value,
      signal,
      operations,
      directory,
      rootIdentity,
    );
    signal?.throwIfAborted();
    await assertDirectoryIdentity(directory, rootIdentity, operations);
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
        directory,
        rootIdentity,
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
  if (usesWindowsNativeHelper(operations)) {
    return resolveWindowsGeneration(absoluteRoot, requiredNames);
  }
  if (usesLinuxSecureTraversal(operations)) {
    return resolveJsonGenerationLinux(absoluteRoot, requiredNames);
  }
  const manifestPath = join(absoluteRoot, "current.json");
  await assertSafePath(absoluteRoot, absoluteRoot, operations);
  const rootIdentity = await captureDirectoryIdentity(
    absoluteRoot,
    operations,
  );
  await assertSafePath(absoluteRoot, manifestPath, operations);
  const manifest = JSON.parse(
    (await readFileNoFollow(
      manifestPath,
      absoluteRoot,
      operations,
      rootIdentity,
    ))
      .toString("utf8"),
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
  await assertSafePath(absoluteRoot, generationPath, operations);
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
    const bytes = await readFileNoFollow(
      artifactPath,
      absoluteRoot,
      operations,
      rootIdentity,
    );
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
    manifestPath,
    paths,
    artifacts,
  };
}

async function writeJsonDurable(
  filePath,
  value,
  signal,
  operations,
  allowedRoot = dirname(resolve(filePath)),
  rootIdentity,
) {
  const absolutePath = resolve(filePath);
  const artifact = basename(absolutePath);
  const bytes = Buffer.from(
    `${JSON.stringify(value, null, 2)}\n`,
    "utf8",
  );
  let handle;
  try {
    await assertDirectoryIdentity(allowedRoot, rootIdentity, operations);
    await assertSafePath(allowedRoot, absolutePath, operations, {
      allowMissing: true,
    });
    handle = await operations.open(
      absolutePath,
      operations.createFileFlags ?? "wx",
    );
    await assertOpenedPathSafe(
      allowedRoot,
      absolutePath,
      handle,
      operations,
      rootIdentity,
    );
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
  if (usesLinuxSecureTraversal(operations)) {
    const handle = await openLinuxDirectoryPath(resolve(directory));
    try {
      await syncLinuxHandle(handle);
    } finally {
      await handle.close();
    }
    return;
  }
  if ((operations.platform ?? "unix") === "win32") {
    if (typeof operations.flushDirectory !== "function") {
      throw new Error(
        "Windows directory synchronization requires the checked-in helper.",
      );
    }
    await operations.flushDirectory(resolve(directory));
    return;
  }

  let directoryHandle;
  try {
    directoryHandle = await operations.open(directory, "r");
  } catch (error) {
    if (!isUnsupportedUnixDirectoryOperation(error)) throw error;
    return;
  }
  try {
    await directoryHandle.sync();
  } catch (error) {
    if (!isUnsupportedUnixDirectoryOperation(error)) throw error;
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
  allowedRoot = filesystemRoot(directory),
) {
  const absolutePath = resolve(directory);
  const absoluteRoot = resolve(allowedRoot);
  assertWithinRoot(absoluteRoot, absolutePath);
  if (usesWindowsNativeHelper(operations)) {
    signal?.throwIfAborted();
    const result = await ensureWindowsDirectory(
      absoluteRoot,
      absolutePath,
    );
    signal?.throwIfAborted();
    return result.rootPath;
  }
  if (usesLinuxSecureTraversal(operations)) {
    const handle = await openLinuxDirectoryPath(absolutePath, {
      create: true,
    });
    await handle.close();
    await syncLinuxDirectoryBoundary(absolutePath);
    return captureDirectoryIdentity(absoluteRoot, operations);
  }
  await assertSafePath(absoluteRoot, absolutePath, operations, {
    allowMissing: true,
  });
  let rootIdentity = await captureDirectoryIdentity(
    absoluteRoot,
    operations,
    { allowMissing: true },
  );
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
    await assertSafePath(absoluteRoot, dirname(path), operations, {
      allowAncestor: true,
    });
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
    await assertSafePath(absoluteRoot, path, operations);
    if (rootIdentity === undefined && resolve(path) === absoluteRoot) {
      rootIdentity = await captureDirectoryIdentity(
        absoluteRoot,
        operations,
      );
    }
    await syncDirectory(durableParent, operations);
    await syncDirectory(path, operations);
    durableParent = path;
  }

  await syncDirectoryBoundary(absolutePath, operations);
  rootIdentity ??= await captureDirectoryIdentity(
    absoluteRoot,
    operations,
  );
  return rootIdentity;
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
    const info = await (operations.lstat ?? operations.stat)(path);
    if (info.isSymbolicLink?.()) {
      throw new Error(
        `Publication path '${path}' cannot be a symbolic link or reparse point.`,
      );
    }
    return info.isDirectory();
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function isUnsupportedUnixDirectoryOperation(error) {
  return error?.code === "ENOTSUP"
    || error?.code === "EOPNOTSUPP";
}

function usesWindowsNativeHelper(operations) {
  return (operations.platform ?? "unix") === "win32"
    && operations.windowsNativeHelper === true;
}

function usesLinuxSecureTraversal(operations) {
  return operations.platform === "linux"
    && operations.linuxSecureTraversal === true;
}

function isSafeArtifactName(artifactName) {
  return typeof artifactName === "string"
    && safeArtifactNamePattern.test(artifactName);
}

async function readFileNoFollow(
  path,
  allowedRoot,
  operations,
  rootIdentity,
) {
  await assertDirectoryIdentity(allowedRoot, rootIdentity, operations);
  await assertSafePath(allowedRoot, path, operations);
  let handle;
  try {
    handle = await operations.open(
      path,
      operations.readFileFlags ?? "r",
    );
    await assertOpenedPathSafe(
      allowedRoot,
      path,
      handle,
      operations,
      rootIdentity,
    );
    return await handle.readFile();
  } finally {
    await handle?.close();
  }
}

async function writeJsonAtomicLinux(
  absolutePath,
  value,
  signal,
  allowedRoot,
) {
  assertWithinRoot(allowedRoot, absolutePath);
  const directory = dirname(absolutePath);
  const directoryHandle = await openLinuxDirectoryPath(directory, {
    create: true,
  });
  try {
    await writeLinuxAtomicFile(
      directoryHandle,
      basename(absolutePath),
      Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8"),
      signal,
    );
  } finally {
    await directoryHandle.close();
  }
}

async function publishJsonArtifactLinux(
  descriptorPath,
  value,
  signal,
  artifact,
) {
  const directory = dirname(descriptorPath);
  const directoryHandle = await openLinuxDirectoryPath(directory, {
    create: true,
  });
  let descriptorPublished = false;
  try {
    const entry = await writeLinuxDurableFile(
      directoryHandle,
      artifact,
      value,
      signal,
    );
    await syncLinuxHandle(directoryHandle);
    await writeLinuxAtomicFile(
      directoryHandle,
      basename(descriptorPath),
      Buffer.from(`${JSON.stringify({
        format: artifactDescriptorFormat,
        version: 1,
        ...entry,
      }, null, 2)}\n`, "utf8"),
      signal,
      () => {
        descriptorPublished = true;
      },
    );
    return {
      commitDescriptorPath: descriptorPath,
      artifactPath: join(directory, artifact),
      ...entry,
    };
  } finally {
    if (!descriptorPublished) {
      await rm(linuxChildPath(directoryHandle, artifact), {
        force: true,
      }).catch(() => {});
    }
    await directoryHandle.close();
  }
}

async function publishJsonGenerationLinux(absoluteRoot, entries, signal) {
  const generation = `${Date.now()}-${process.pid}-${randomUUID()}`;
  const rootHandle = await openLinuxDirectoryPath(absoluteRoot, {
    create: true,
  });
  let generationsHandle;
  let generationHandle;
  let pointerPublished = false;
  try {
    generationsHandle = await openLinuxChildDirectory(
      rootHandle,
      "generations",
      { create: true },
    );
    generationHandle = await openLinuxChildDirectory(
      generationsHandle,
      generation,
      { create: true },
    );
    const writes = await Promise.allSettled(
      entries.map(([name, value]) =>
        writeLinuxDurableFile(
          generationHandle,
          `${name}.json`,
          value,
          signal,
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
    await syncLinuxHandle(generationHandle);
    await syncLinuxHandle(generationsHandle);
    await syncLinuxHandle(rootHandle);
    const manifest = {
      format: generationManifestFormat,
      version: 1,
      generation,
      files: Object.fromEntries(
        entries.map(([name], index) => [name, writes[index].value]),
      ),
    };
    await writeLinuxAtomicFile(
      rootHandle,
      "current.json",
      Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`, "utf8"),
      signal,
      () => {
        pointerPublished = true;
      },
    );
    return {
      rootPath: absoluteRoot,
      generation,
      generationPath: join(absoluteRoot, "generations", generation),
      manifestPath: join(absoluteRoot, "current.json"),
      paths: Object.fromEntries(
        entries.map(([name]) => [
          name,
          join(absoluteRoot, "generations", generation, `${name}.json`),
        ]),
      ),
    };
  } finally {
    if (!pointerPublished && generationHandle !== undefined) {
      for (const [name] of entries) {
        await rm(linuxChildPath(generationHandle, `${name}.json`), {
          force: true,
        }).catch(() => {});
      }
    }
    await generationHandle?.close();
    if (!pointerPublished && generationsHandle !== undefined) {
      await rm(linuxChildPath(generationsHandle, generation), {
        recursive: true,
        force: true,
      }).catch(() => {});
    }
    await generationsHandle?.close();
    await rootHandle.close();
  }
}

async function resolveJsonGenerationLinux(absoluteRoot, requiredNames) {
  const rootHandle = await openLinuxDirectoryPath(absoluteRoot);
  let generationsHandle;
  let generationHandle;
  try {
    const manifest = JSON.parse(
      (await readLinuxFile(rootHandle, "current.json")).toString("utf8"),
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
    generationsHandle = await openLinuxChildDirectory(
      rootHandle,
      "generations",
    );
    generationHandle = await openLinuxChildDirectory(
      generationsHandle,
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
      const bytes = await readLinuxFile(generationHandle, entry.artifact);
      const digest = createHash("sha256").update(bytes).digest("hex");
      if (
        bytes.byteLength !== entry.byteLength
        || digest !== entry.sha256
      ) {
        throw new Error(
          `Bootstrap generation artifact '${name}' does not match its manifest.`,
        );
      }
      paths[name] = join(
        absoluteRoot,
        "generations",
        manifest.generation,
        entry.artifact,
      );
      artifacts[name] = entry;
    }
    return {
      rootPath: absoluteRoot,
      generation: manifest.generation,
      generationPath: join(
        absoluteRoot,
        "generations",
        manifest.generation,
      ),
      manifestPath: join(absoluteRoot, "current.json"),
      paths,
      artifacts,
    };
  } finally {
    await generationHandle?.close();
    await generationsHandle?.close();
    await rootHandle.close();
  }
}

async function writeLinuxDurableFile(
  directoryHandle,
  name,
  value,
  signal,
) {
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
  let fileHandle;
  try {
    signal?.throwIfAborted();
    fileHandle = await open(
      linuxChildPath(directoryHandle, name),
      linuxCreateFileFlags(),
    );
    await fileHandle.writeFile(bytes, { signal });
    signal?.throwIfAborted();
    await fileHandle.sync();
  } finally {
    await fileHandle?.close();
  }
  return {
    artifact: name,
    byteLength: bytes.byteLength,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

async function writeLinuxAtomicFile(
  directoryHandle,
  destinationName,
  bytes,
  signal,
  onCommitted = () => {},
) {
  const stagingName =
    `${destinationName}.${process.pid}.${randomUUID()}.tmp`;
  const stagingPath = linuxChildPath(directoryHandle, stagingName);
  let stagingHandle;
  let renamed = false;
  try {
    signal?.throwIfAborted();
    stagingHandle = await open(stagingPath, linuxCreateFileFlags());
    await stagingHandle.writeFile(bytes, { signal });
    signal?.throwIfAborted();
    await stagingHandle.sync();
    await stagingHandle.close();
    stagingHandle = undefined;
    signal?.throwIfAborted();
    await assertLinuxDestinationNotLink(directoryHandle, destinationName);
    await rename(
      stagingPath,
      linuxChildPath(directoryHandle, destinationName),
    );
    renamed = true;
    onCommitted();
    await syncLinuxHandle(directoryHandle);
  } catch (error) {
    if (renamed) error.atomicRenameCompleted = true;
    throw error;
  } finally {
    await stagingHandle?.close().catch(() => {});
    await rm(stagingPath, { force: true }).catch(() => {});
  }
}

async function readLinuxFile(directoryHandle, name) {
  let fileHandle;
  try {
    fileHandle = await open(
      linuxChildPath(directoryHandle, name),
      linuxReadFileFlags(),
    );
    return await fileHandle.readFile();
  } finally {
    await fileHandle?.close();
  }
}

async function openLinuxDirectoryPath(path, { create = false } = {}) {
  const absolutePath = resolve(path);
  const root = filesystemRoot(absolutePath);
  let current = await open(root, linuxDirectoryFlags());
  try {
    const relativePath = relative(root, absolutePath);
    const components = relativePath.length === 0
      ? []
      : relativePath.split(sep);
    for (const component of components) {
      const next = await openLinuxChildDirectory(
        current,
        component,
        { create },
      );
      await current.close();
      current = next;
    }
    return current;
  } catch (error) {
    await current.close();
    throw error;
  }
}

async function openLinuxChildDirectory(
  parentHandle,
  name,
  { create = false } = {},
) {
  validateLinuxName(name);
  const path = linuxChildPath(parentHandle, name);
  try {
    return await open(path, linuxDirectoryFlags());
  } catch (error) {
    if (["ELOOP", "ENOTDIR"].includes(error?.code)) {
      throw linuxLinkError(path, error);
    }
    if (!create || error?.code !== "ENOENT") throw error;
  }
  try {
    await mkdir(path, { recursive: false });
  } catch (error) {
    if (error?.code !== "EEXIST") throw error;
  }
  const childHandle = await open(path, linuxDirectoryFlags());
  await syncLinuxHandle(parentHandle);
  await syncLinuxHandle(childHandle);
  return childHandle;
}

async function assertLinuxDestinationNotLink(directoryHandle, name) {
  const path = linuxChildPath(directoryHandle, name);
  try {
    if ((await lstat(path)).isSymbolicLink()) {
      throw linuxLinkError(path);
    }
  } catch (error) {
    if (error?.code === "ENOENT") return;
    throw error;
  }
}

function linuxLinkError(path, cause) {
  const error = new Error(
    `Linux path '${path}' contains a symbolic link or non-directory component.`,
    cause === undefined ? undefined : { cause },
  );
  error.code = "symbolic_link";
  return error;
}

async function syncLinuxDirectoryBoundary(path) {
  const parentPath = dirname(path);
  if (parentPath !== path) {
    const parentHandle = await openLinuxDirectoryPath(parentPath);
    try {
      await syncLinuxHandle(parentHandle);
    } finally {
      await parentHandle.close();
    }
  }
  const handle = await openLinuxDirectoryPath(path);
  try {
    await syncLinuxHandle(handle);
  } finally {
    await handle.close();
  }
}

async function syncLinuxHandle(handle) {
  try {
    await handle.sync();
  } catch (error) {
    if (!isUnsupportedUnixDirectoryOperation(error)) throw error;
  }
}

function linuxChildPath(directoryHandle, name) {
  validateLinuxName(name);
  return join("/proc/self/fd", String(directoryHandle.fd), name);
}

function validateLinuxName(name) {
  if (
    typeof name !== "string"
    || name.length === 0
    || name === "."
    || name === ".."
    || basename(name) !== name
  ) {
    throw new Error(`Linux path component '${name}' is invalid.`);
  }
}

function linuxDirectoryFlags() {
  return constants.O_RDONLY
    | constants.O_DIRECTORY
    | constants.O_NOFOLLOW
    | (constants.O_CLOEXEC ?? 0);
}

function linuxCreateFileFlags() {
  return constants.O_WRONLY
    | constants.O_CREAT
    | constants.O_EXCL
    | constants.O_NOFOLLOW
    | (constants.O_CLOEXEC ?? 0);
}

function linuxReadFileFlags() {
  return constants.O_RDONLY
    | constants.O_NOFOLLOW
    | (constants.O_CLOEXEC ?? 0);
}

async function assertSafePath(
  allowedRoot,
  path,
  operations,
  { allowMissing = false, allowAncestor = false } = {},
) {
  const root = resolve(allowedRoot);
  const target = resolve(path);
  if (allowAncestor) {
    assertRelatedPath(root, target);
  } else {
    assertWithinRoot(root, target);
  }
  if (typeof operations.lstat !== "function") return;

  let cursor = target;
  const components = [];
  while (true) {
    components.push(cursor);
    const parent = dirname(cursor);
    if (parent === cursor) break;
    cursor = parent;
  }

  let missing = false;
  for (const component of components.reverse()) {
    let info;
    try {
      info = await operations.lstat(component);
    } catch (error) {
      if (error?.code === "ENOENT" && allowMissing) {
        missing = true;
        continue;
      }
      throw error;
    }
    if (missing) {
      throw new Error(
        `Publication path '${target}' has an existing child below a missing parent.`,
      );
    }
    if (info.isSymbolicLink?.()) {
      throw new Error(
        `Publication path '${component}' cannot be a symbolic link or reparse point.`,
      );
    }
  }

  if (
    (operations.platform ?? "unix") === "win32"
    && typeof operations.validateWindowsPath === "function"
  ) {
    await operations.validateWindowsPath(target);
  }

  if (!allowAncestor) {
    await assertRealPathContained(root, target, operations, allowMissing);
  }
}

async function assertOpenedPathSafe(
  allowedRoot,
  path,
  handle,
  operations,
  rootIdentity,
) {
  await assertDirectoryIdentity(allowedRoot, rootIdentity, operations);
  await assertSafePath(allowedRoot, path, operations);
  const platform = operations.platform;
  if (platform === undefined || platform === "win32") return;
  if (platform !== "linux") {
    throw new Error(
      "Secure opened-descriptor containment is supported only on Linux " +
      "and Windows.",
    );
  }
  if (
    !Number.isInteger(handle?.fd)
    || typeof operations.realpath !== "function"
  ) {
    throw new Error(
      "Linux opened-descriptor containment requires an fd and realpath.",
    );
  }
  const openedPath = await operations.realpath(`/proc/self/fd/${handle.fd}`);
  assertWithinRoot(await canonicalRoot(allowedRoot, operations), openedPath);
}

async function captureDirectoryIdentity(
  path,
  operations,
  { allowMissing = false } = {},
) {
  if (typeof operations.lstat !== "function") return undefined;
  let info;
  try {
    info = await operations.lstat(resolve(path), { bigint: true });
  } catch (error) {
    if (allowMissing && error?.code === "ENOENT") return undefined;
    throw error;
  }
  if (info.isSymbolicLink?.() || !info.isDirectory()) {
    throw new Error(
      `Publication root '${resolve(path)}' must be a non-link directory.`,
    );
  }
  if (info.dev === undefined || info.ino === undefined) return undefined;
  return `${String(info.dev)}:${String(info.ino)}`;
}

async function assertDirectoryIdentity(
  path,
  expectedIdentity,
  operations,
) {
  if (expectedIdentity === undefined) return;
  const currentIdentity = await captureDirectoryIdentity(path, operations);
  if (currentIdentity !== expectedIdentity) {
    throw new Error(
      `Publication root '${resolve(path)}' changed directory identity.`,
    );
  }
}

async function assertRealPathContained(
  allowedRoot,
  path,
  operations,
  allowMissing,
) {
  if (typeof operations.realpath !== "function") return;
  let realTarget;
  try {
    realTarget = await operations.realpath(path);
  } catch (error) {
    if (allowMissing && error?.code === "ENOENT") return;
    throw error;
  }
  assertWithinRoot(await canonicalRoot(allowedRoot, operations), realTarget);
}

async function canonicalRoot(root, operations) {
  try {
    return await operations.realpath(root);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
    let cursor = resolve(root);
    const suffix = [];
    while (true) {
      const parent = dirname(cursor);
      if (parent === cursor) throw error;
      suffix.unshift(basename(cursor));
      cursor = parent;
      try {
        return resolve(await operations.realpath(cursor), ...suffix);
      } catch (ancestorError) {
        if (ancestorError?.code !== "ENOENT") throw ancestorError;
      }
    }
  }
}

function assertWithinRoot(allowedRoot, path) {
  const root = resolve(allowedRoot);
  const target = resolve(path);
  const rel = relative(root, target);
  if (
    rel === ".."
    || rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`)
    || isAbsolute(rel)
  ) {
    throw new Error(
      `Publication path '${target}' escapes configured root '${root}'.`,
    );
  }
}

function assertRelatedPath(allowedRoot, path) {
  try {
    assertWithinRoot(allowedRoot, path);
  } catch {
    assertWithinRoot(path, allowedRoot);
  }
}

function filesystemRoot(path) {
  let cursor = resolve(path);
  while (dirname(cursor) !== cursor) cursor = dirname(cursor);
  return cursor;
}
