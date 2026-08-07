import { createHash, randomUUID } from "node:crypto";
import {
  mkdir,
  open,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  createDirectoryDurable,
  publishJsonArtifact,
  publishJsonGeneration,
  resolveJsonGeneration,
  writeJsonAtomic,
} from "../../../examples/tetris-integration/durable-json.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const realOperations = { mkdir, open, readFile, rename, rm, stat };

describe("Tetris durable atomic JSON writes", () => {
  it("syncs and closes staging before rename, then syncs the directory", async () => {
    const events = [];
    const operations = fakeOperations(events);

    await writeJsonAtomic("state.json", { version: 1 }, undefined, operations);

    expect(events).toEqual([
      "stat-directory",
      "open-directory",
      "sync-directory",
      "close-directory",
      "open-directory",
      "sync-directory",
      "close-directory",
      "open-staging",
      "write-staging",
      "sync-staging",
      "close-staging",
      "rename",
      "open-directory",
      "sync-directory",
      "close-directory",
      "remove-staging",
    ]);
  });

  it("does not rename and removes staging when durable staging sync fails", async () => {
    const events = [];
    const syncError = new Error("sync failed");
    const operations = fakeOperations(events, { stagingSyncError: syncError });

    await expect(
      writeJsonAtomic("state.json", { version: 1 }, undefined, operations),
    ).rejects.toBe(syncError);

    expect(events).not.toContain("rename");
    expect(events).toEqual([
      "stat-directory",
      "open-directory",
      "sync-directory",
      "close-directory",
      "open-directory",
      "sync-directory",
      "close-directory",
      "open-staging",
      "write-staging",
      "sync-staging",
      "close-staging",
      "remove-staging",
    ]);
  });

  it("retains atomic rename when directory fsync is unsupported", async () => {
    const events = [];
    const unsupported = Object.assign(new Error("unsupported"), {
      code: "ENOTSUP",
    });
    const operations = fakeOperations(events, {
      directorySyncError: unsupported,
    });

    await writeJsonAtomic("state.json", { version: 1 }, undefined, operations);

    expect(events).toContain("rename");
    expect(events.at(-1)).toBe("remove-staging");
  });

  it.each(["EACCES", "EPERM"])(
    "surfaces a directory sync permission failure: %s",
    async (code) => {
      const events = [];
      const permissionError = Object.assign(new Error("permission denied"), {
        code,
      });
      const operations = fakeOperations(events, {
        directoryOpenError: permissionError,
        directoryOpenErrorAt: 3,
      });

      await expect(
        writeJsonAtomic("state.json", { version: 1 }, undefined, operations),
      ).rejects.toBe(permissionError);
      expect(permissionError.atomicRenameCompleted).toBe(true);
    },
  );

  it("creates a missing tree with parent-before-child sync ordering", async () => {
    const ancestor = artifactPath("durable-tree-ancestor");
    const child = resolve(ancestor, "first");
    const target = resolve(child, "second");
    const events = [];
    const operations = recordingDirectoryOperations(ancestor, events);

    await createDirectoryDurable(target, undefined, operations);

    expect(events).toEqual([
      `sync:${dirname(ancestor)}`,
      `sync:${ancestor}`,
      `mkdir:${child}`,
      `sync:${ancestor}`,
      `sync:${child}`,
      `mkdir:${target}`,
      `sync:${child}`,
      `sync:${target}`,
      `sync:${child}`,
      `sync:${target}`,
    ]);
  });

  it("syncs the requested boundary for an existing directory", async () => {
    const root = artifactPath("durable-existing");
    const events = [];
    const operations = recordingDirectoryOperations(root, events);

    await createDirectoryDurable(root, undefined, operations);

    expect(events).toEqual([
      `sync:${dirname(root)}`,
      `sync:${root}`,
    ]);
  });

  it("syncs a filesystem or volume root once", async () => {
    let root = repositoryRoot;
    while (dirname(root) !== root) root = dirname(root);
    const events = [];
    const operations = recordingDirectoryOperations(root, events);

    await createDirectoryDurable(root, undefined, operations);

    expect(events).toEqual([`sync:${root}`]);
  });

  it("concurrent durable directory creators converge", async () => {
    const root = artifactPath("durable-concurrent");
    const target = resolve(root, "first", "second");
    try {
      await Promise.all(
        Array.from(
          { length: 8 },
          () => createDirectoryDurable(target),
        ),
      );
      expect((await stat(target)).isDirectory()).toBe(true);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("stabilizes a concurrent intermediate ancestor before descendants", async () => {
    for (let attempt = 0; attempt < 10; attempt += 1) {
      const ancestor = artifactPath(`durable-intermediate-hook-${attempt}`);
      const intermediate = resolve(ancestor, "first");
      const target = resolve(intermediate, "second");
      const directories = new Set([ancestor]);
      const created = deferred();
      const releaseCreator = deferred();
      const creatorAOperations = recordingDirectoryOperations(
        ancestor,
        [],
        {
          directories,
          async afterMkdir() {
            created.resolve();
            await releaseCreator.promise;
          },
        },
      );
      const creatorBEvents = [];
      const creatorBOperations = recordingDirectoryOperations(
        ancestor,
        creatorBEvents,
        { directories },
      );
      const creatorA = createDirectoryDurable(
        intermediate,
        undefined,
        creatorAOperations,
      );

      await created.promise;
      try {
        await createDirectoryDurable(target, undefined, creatorBOperations);
        expect(creatorBEvents).toEqual([
          `sync:${ancestor}`,
          `sync:${intermediate}`,
          `mkdir:${target}`,
          `sync:${intermediate}`,
          `sync:${target}`,
          `sync:${intermediate}`,
          `sync:${target}`,
        ]);
      } finally {
        releaseCreator.resolve();
        await creatorA;
      }
    }
  });

  it("an existing concurrent directory receives its boundary sync", async () => {
    for (let attempt = 0; attempt < 10; attempt += 1) {
      const ancestor = artifactPath(`durable-hook-${attempt}`);
      const target = resolve(ancestor, "target");
      const directories = new Set([ancestor]);
      const created = deferred();
      const releaseCreator = deferred();
      const creatorAEvents = [];
      const creatorBEvents = [];
      const creatorAOperations = recordingDirectoryOperations(
        ancestor,
        creatorAEvents,
        {
          directories,
          async afterMkdir() {
            created.resolve();
            await releaseCreator.promise;
          },
        },
      );
      const creatorBOperations = recordingDirectoryOperations(
        ancestor,
        creatorBEvents,
        { directories },
      );
      const creatorA = createDirectoryDurable(
        target,
        undefined,
        creatorAOperations,
      );

      await created.promise;
      try {
        await createDirectoryDurable(target, undefined, creatorBOperations);
        expect(creatorBEvents).toEqual([
          `sync:${ancestor}`,
          `sync:${target}`,
        ]);
      } finally {
        releaseCreator.resolve();
        await creatorA;
      }
    }
  });

  it("propagates a concurrent boundary failure repeatedly", async () => {
    for (let attempt = 0; attempt < 10; attempt += 1) {
      const ancestor = artifactPath(`durable-hook-failure-${attempt}`);
      const target = resolve(ancestor, "target");
      const directories = new Set([ancestor]);
      const created = deferred();
      const releaseCreator = deferred();
      const creatorAOperations = recordingDirectoryOperations(
        ancestor,
        [],
        {
          directories,
          async afterMkdir() {
            created.resolve();
            await releaseCreator.promise;
          },
        },
      );
      const events = [];
      const expected = new Error("injected boundary sync failure");
      const failurePath = attempt % 2 === 0 ? ancestor : target;
      const creatorBOperations = recordingDirectoryOperations(
        ancestor,
        events,
        {
          directories,
          syncFailure(path) {
            return path === failurePath ? expected : undefined;
          },
        },
      );
      const creatorA = createDirectoryDurable(
        target,
        undefined,
        creatorAOperations,
      );

      await created.promise;
      try {
        await expect(createDirectoryDurable(
          target,
          undefined,
          creatorBOperations,
        )).rejects.toBe(expected);
        expect(events).toEqual(
          failurePath === ancestor
            ? [`sync:${ancestor}`]
            : [`sync:${ancestor}`, `sync:${target}`],
        );
      } finally {
        releaseCreator.resolve();
        await creatorA;
      }
    }
  });

  it("does not publish a descriptor when parent sync fails after mkdir", async () => {
    const parent = artifactPath("direct-create-parent");
    const directory = resolve(parent, "nested");
    const descriptorPath = resolve(directory, "state.commit.json");
    await mkdir(parent, { recursive: true });
    let directoryOpenCount = 0;
    const operations = {
      ...realOperations,
      async open(path, mode) {
        const handle = await open(path, mode);
        if (mode !== "r" || ++directoryOpenCount !== 3) return handle;
        return {
          async sync() {
            throw new Error("injected parent sync failure");
          },
          close: () => handle.close(),
        };
      },
    };
    try {
      await expect(publishJsonArtifact(
        descriptorPath,
        { version: 1 },
        undefined,
        operations,
      )).rejects.toThrow("injected parent sync failure");

      await expect(readFile(descriptorPath))
        .rejects.toMatchObject({ code: "ENOENT" });
      expect(await readdir(directory)).toEqual([]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });

  it("publishes receipt, state, and evidence as one durable generation", async () => {
    const root = artifactPath("bootstrap-generation");
    try {
      const published = await publishJsonGeneration(
        root,
        {
          receipt: { revision: "one" },
          state: { value: 800 },
          evidence: { quality: 0.82 },
        },
      );
      const resolved = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );

      expect(resolved.generation).toBe(published.generation);
      expect(JSON.parse(await readFile(resolved.paths.receipt, "utf8")))
        .toEqual({ revision: "one" });
      expect(JSON.parse(await readFile(resolved.paths.state, "utf8")))
        .toEqual({ value: 800 });
      expect(JSON.parse(await readFile(resolved.paths.evidence, "utf8")))
        .toEqual({ quality: 0.82 });
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("publishes a reusable direct commit descriptor last", async () => {
    const root = artifactPath("direct-commit");
    try {
      const descriptorPath = resolve(root, "state.commit.json");
      const publication = await publishJsonArtifact(
        descriptorPath,
        { version: 1, value: 800 },
      );
      const descriptor = JSON.parse(await readFile(descriptorPath, "utf8"));
      const bytes = await readFile(publication.artifactPath);

      expect(descriptor).toEqual({
        format: "flaggo.committed-artifact",
        version: 1,
        artifact: basename(publication.artifactPath),
        byteLength: bytes.byteLength,
        sha256: createHash("sha256").update(bytes).digest("hex"),
      });
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it.each([
    ["before-artifact-directory-sync", "old"],
    ["before-descriptor-rename", "old"],
    ["after-descriptor-rename", "new"],
  ])(
    "keeps a usable direct descriptor after failure %s",
    async (stage, expectedGeneration) => {
      const root = artifactPath(`direct-ordering-${stage}`);
      const descriptorPath = resolve(root, "state.commit.json");
      try {
        await publishJsonArtifact(descriptorPath, { generation: "old" });
        const events = [];

        await expect(publishJsonArtifact(
          descriptorPath,
          { generation: "new" },
          undefined,
          failingDirectPublicationOperations(stage, events),
        )).rejects.toThrow(`injected ${stage} failure`);

        expect(await resolveDirectJson(descriptorPath))
          .toEqual({ generation: expectedGeneration });
        if (stage === "before-descriptor-rename") {
          expect(events.indexOf("artifact-directory-sync"))
            .toBeLessThan(events.indexOf("descriptor-rename"));
        }
        if (stage === "after-descriptor-rename") {
          expect(events).toEqual([
            "artifact-directory-sync",
            "descriptor-rename",
            "descriptor-directory-sync",
          ]);
        }
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.each([
    "-state",
    "_state",
    "state/escape",
    "state\\escape",
    "st\u00e1te",
    "a".repeat(91),
    "a".repeat(129),
  ])("rejects unsafe generated artifact name before publication: %s", async (artifactStem) => {
    const root = artifactPath("direct-invalid-name");
    await mkdir(root, { recursive: true });
    try {
      await expect(publishJsonArtifact(
        resolve(root, "state.commit.json"),
        { version: 1 },
        undefined,
        realOperations,
        artifactStem,
      )).rejects.toBeInstanceOf(TypeError);

      expect(await readdir(root)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("accepts the 128-character artifact filename boundary", async () => {
    const root = artifactPath("direct-boundary-name");
    try {
      const publication = await publishJsonArtifact(
        resolve(root, "state.commit.json"),
        { version: 1 },
        undefined,
        realOperations,
        "a".repeat(90),
      );

      expect(basename(publication.artifactPath)).toHaveLength(128);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("rejects an in-place generation artifact mutation", async () => {
    const root = artifactPath("bootstrap-digest");
    try {
      const publication = await publishJsonGeneration(
        root,
        generationFiles("old"),
      );
      const original = await readFile(publication.paths.state, "utf8");
      await writeFile(
        publication.paths.state,
        original.replace('"old"', '"bad"'),
        "utf8",
      );

      await expect(resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      )).rejects.toThrow("does not match its manifest");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("keeps the old generation visible until the single pointer switch", async () => {
    const root = artifactPath("bootstrap-switch");
    const gate = deferred();
    const renameStarted = deferred();
    try {
      const oldGeneration = await publishJsonGeneration(
        root,
        generationFiles("old"),
      );
      const delayedOperations = {
        ...realOperations,
        async rename(source, destination) {
          if (basename(destination) === "current.json") {
            renameStarted.resolve();
            await gate.promise;
          }
          return rename(source, destination);
        },
      };
      const publication = publishJsonGeneration(
        root,
        generationFiles("new"),
        undefined,
        delayedOperations,
      );
      await renameStarted.promise;

      const beforeSwitch = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(beforeSwitch.generation).toBe(oldGeneration.generation);
      expect(JSON.parse(await readFile(beforeSwitch.paths.state, "utf8")))
        .toEqual({ generation: "old" });

      gate.resolve();
      const newGeneration = await publication;
      const afterSwitch = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(afterSwitch.generation).toBe(newGeneration.generation);
      expect(JSON.parse(await readFile(afterSwitch.paths.state, "utf8")))
        .toEqual({ generation: "new" });
    } finally {
      gate.resolve();
      await rm(root, { recursive: true, force: true });
    }
  });

  it("drains sibling writes before cleaning a failed generation", async () => {
    const root = artifactPath("bootstrap-drain");
    const events = [];
    try {
      const operations = failingGenerationOperations(root, "file-write", events, {
        delayedSibling: true,
      });
      await expect(publishJsonGeneration(
        root,
        generationFiles("failed"),
        undefined,
        operations,
      )).rejects.toBeInstanceOf(AggregateError);

      expect(events.indexOf("delayed-close")).toBeLessThan(
        events.indexOf("remove-generation"),
      );
      expect(await generationNames(root)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it.each([
    "generation-mkdir",
    "file-open",
    "file-write",
    "file-sync",
    "generation-sync",
    "generations-sync",
    "root-pre-manifest-sync",
    "manifest-write",
    "manifest-sync",
    "manifest-rename",
  ])("preserves the old generation and cleans pre-switch failure: %s", async (stage) => {
    const root = artifactPath(`bootstrap-failure-${stage}`);
    try {
      const oldGeneration = await publishJsonGeneration(
        root,
        generationFiles("old"),
      );
      await expect(publishJsonGeneration(
        root,
        generationFiles("new"),
        undefined,
        failingGenerationOperations(root, stage, []),
      )).rejects.toThrow();

      const resolved = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(resolved.generation).toBe(oldGeneration.generation);
      expect(await generationNames(root)).toEqual([oldGeneration.generation]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("does not publish current when the generations parent sync fails", async () => {
    const root = artifactPath("bootstrap-generations-sync");
    const events = [];
    try {
      const oldGeneration = await publishJsonGeneration(
        root,
        generationFiles("old"),
      );
      await expect(publishJsonGeneration(
        root,
        generationFiles("new"),
        undefined,
        failingGenerationOperations(root, "generations-sync", events),
      )).rejects.toThrow("injected generations-sync failure");

      expect(events).toContain("generations-sync");
      expect(events).not.toContain("manifest-rename");
      const resolved = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(resolved.generation).toBe(oldGeneration.generation);
      expect(await generationNames(root)).toEqual([oldGeneration.generation]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("does not publish the first current pointer before syncing the root entry", async () => {
    const root = artifactPath("bootstrap-first-root-sync");
    const events = [];
    try {
      await expect(publishJsonGeneration(
        root,
        generationFiles("first"),
        undefined,
        failingGenerationOperations(root, "root-pre-manifest-sync", events),
      )).rejects.toThrow("injected root-pre-manifest-sync failure");

      expect(events).toContain("generations-sync");
      expect(events).toContain("root-pre-manifest-sync");
      expect(events).not.toContain("manifest-rename");
      await expect(readFile(resolve(root, "current.json")))
        .rejects.toMatchObject({ code: "ENOENT" });
      expect(await generationNames(root)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("syncs the existing root before and after switching current", async () => {
    const root = artifactPath("bootstrap-existing-root-sync");
    const events = [];
    try {
      await publishJsonGeneration(root, generationFiles("old"));
      const publication = await publishJsonGeneration(
        root,
        generationFiles("new"),
        undefined,
        failingGenerationOperations(root, undefined, events),
      );

      expect(events).toEqual([
        "root-pre-manifest-sync",
        "generations-sync",
        "generations-sync",
        "generations-sync",
        "manifest-rename",
        "root-post-manifest-sync",
      ]);
      const resolved = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(resolved.generation).toBe(publication.generation);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("retains the switched generation when root directory sync fails after rename", async () => {
    const root = artifactPath("bootstrap-post-switch-sync");
    try {
      const oldGeneration = await publishJsonGeneration(
        root,
        generationFiles("old"),
      );
      await expect(publishJsonGeneration(
        root,
        generationFiles("new"),
        undefined,
        failingGenerationOperations(root, "root-post-manifest-sync", []),
      )).rejects.toThrow("injected root-post-manifest-sync failure");

      const resolved = await resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      );
      expect(resolved.generation).not.toBe(oldGeneration.generation);
      expect(JSON.parse(await readFile(resolved.paths.state, "utf8")))
        .toEqual({ generation: "new" });
      expect((await generationNames(root)).length).toBe(2);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

function artifactPath(prefix) {
  return resolve(
    repositoryRoot,
    ".flaggo",
    "test-artifacts",
    `${prefix}-${randomUUID()}`,
  );
}

function generationFiles(generation) {
  return {
    receipt: { generation },
    state: { generation },
    evidence: { generation },
  };
}

async function generationNames(root) {
  try {
    return (await readdir(resolve(root, "generations"))).sort();
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
}

async function resolveDirectJson(descriptorPath) {
  const descriptor = JSON.parse(await readFile(descriptorPath, "utf8"));
  const bytes = await readFile(resolve(dirname(descriptorPath), descriptor.artifact));
  expect(bytes.byteLength).toBe(descriptor.byteLength);
  expect(createHash("sha256").update(bytes).digest("hex"))
    .toBe(descriptor.sha256);
  return JSON.parse(bytes.toString("utf8"));
}

function failingDirectPublicationOperations(stage, events) {
  let directorySyncCount = 0;
  return {
    ...realOperations,
    async open(path, mode) {
      const handle = await open(path, mode);
      if (mode !== "r") return handle;
      directorySyncCount += 1;
      const syncEvent = directorySyncCount === 3
        ? "artifact-directory-sync"
        : directorySyncCount === 6
          ? "descriptor-directory-sync"
          : undefined;
      if (syncEvent === undefined) return handle;
      return {
        async sync() {
          events.push(syncEvent);
          if (
            stage === "before-artifact-directory-sync"
            && syncEvent === "artifact-directory-sync"
          ) {
            throw new Error(`injected ${stage} failure`);
          }
          if (
            stage === "after-descriptor-rename"
            && syncEvent === "descriptor-directory-sync"
          ) {
            throw new Error(`injected ${stage} failure`);
          }
          return handle.sync();
        },
        close: () => handle.close(),
      };
    },
    async rename(source, destination) {
      if (basename(destination) === "state.commit.json") {
        events.push("descriptor-rename");
        if (stage === "before-descriptor-rename") {
          throw new Error(`injected ${stage} failure`);
        }
      }
      return rename(source, destination);
    },
  };
}

function failingGenerationOperations(
  root,
  stage,
  events,
  { delayedSibling = false } = {},
) {
  let generationFileCount = 0;
  let preManifestRootSyncCount = 0;
  let manifestRenamed = false;
  const creationSyncPaths = [];
  return {
    ...realOperations,
    async mkdir(path, options) {
      if (
        stage === "generations-mkdir"
        && basename(String(path)) === "generations"
      ) {
        throw new Error("injected generations-mkdir failure");
      }
      if (
        stage === "generation-mkdir"
        && basename(dirname(String(path))) === "generations"
      ) {
        throw new Error("injected generation-mkdir failure");
      }
      const result = await mkdir(path, options);
      creationSyncPaths.push(
        resolve(dirname(String(path))),
        resolve(String(path)),
      );
      return result;
    },
    async open(path, mode) {
      const pathText = String(path);
      const absolutePath = resolve(pathText);
      const creationSync = mode === "r"
        && creationSyncPaths[0] === absolutePath;
      if (creationSync) creationSyncPaths.shift();
      const manifestStaging = basename(pathText).startsWith("current.json.");
      const generationDirectory = mode === "r"
        && basename(dirname(pathText)) === "generations";
      const generationsDirectory = mode === "r"
        && basename(pathText) === "generations";
      const rootDirectory = mode === "r"
        && resolve(pathText) === resolve(root);
      const generationFile = mode === "wx"
        && !manifestStaging
        && basename(dirname(dirname(pathText))) === "generations";

      if (stage === "file-open" && generationFile) {
        throw new Error("injected file-open failure");
      }
      const handle = await open(path, mode);
      if (mode === "r") {
        return {
          async sync() {
            if (creationSync) return handle.sync();
            if (stage === "generation-sync" && generationDirectory) {
              throw new Error("injected generation-sync failure");
            }
            if (generationsDirectory) {
              events.push("generations-sync");
              if (stage === "generations-sync") {
                throw new Error("injected generations-sync failure");
              }
            }
            if (rootDirectory) {
              const rootStage = manifestRenamed
                ? "root-post-manifest-sync"
                : ++preManifestRootSyncCount === 1
                  ? "root-pre-manifest-sync"
                  : undefined;
              if (rootStage === undefined) return handle.sync();
              events.push(rootStage);
              if (stage === rootStage) {
                throw new Error(`injected ${rootStage} failure`);
              }
            }
            return handle.sync();
          },
          close: () => handle.close(),
        };
      }

      generationFileCount += generationFile ? 1 : 0;
      const fileIndex = generationFileCount;
      const delayed = delayedSibling && generationFile && fileIndex === 2;
      return {
        async writeFile(...args) {
          if (delayed) {
            await new Promise((resolvePromise) => setTimeout(resolvePromise, 30));
          }
          if (
            stage === "file-write"
            && generationFile
            && (!delayedSibling || fileIndex === 1)
          ) {
            throw new Error("injected file-write failure");
          }
          if (stage === "manifest-write" && manifestStaging) {
            throw new Error("injected manifest-write failure");
          }
          return handle.writeFile(...args);
        },
        async sync() {
          if (stage === "file-sync" && generationFile) {
            throw new Error("injected file-sync failure");
          }
          if (stage === "manifest-sync" && manifestStaging) {
            throw new Error("injected manifest-sync failure");
          }
          return handle.sync();
        },
        async close() {
          await handle.close();
          if (delayed) events.push("delayed-close");
        },
      };
    },
    async rename(source, destination) {
      if (basename(destination) === "current.json") {
        events.push("manifest-rename");
      }
      if (
        stage === "manifest-rename"
        && basename(destination) === "current.json"
      ) {
        throw new Error("injected manifest-rename failure");
      }
      const result = await rename(source, destination);
      if (basename(destination) === "current.json") {
        manifestRenamed = true;
      }
      return result;
    },
    async rm(path, options) {
      if (
        basename(dirname(String(path))) === "generations"
        && options?.recursive === true
      ) {
        events.push("remove-generation");
      }
      return rm(path, options);
    },
  };
}

function deferred() {
  let resolvePromise;
  const promise = new Promise((resolve) => {
    resolvePromise = resolve;
  });
  return { promise, resolve: resolvePromise };
}

function recordingDirectoryOperations(
  existingAncestor,
  events,
  {
    directories = new Set(),
    afterMkdir,
    syncFailure,
  } = {},
) {
  directories.add(resolve(existingAncestor));
  return {
    async stat(path) {
      if (directories.has(resolve(path))) {
        return { isDirectory: () => true };
      }
      throw Object.assign(new Error("missing"), { code: "ENOENT" });
    },
    async mkdir(path) {
      const absolutePath = resolve(path);
      events.push(`mkdir:${absolutePath}`);
      directories.add(absolutePath);
      await afterMkdir?.(absolutePath);
    },
    async open(path) {
      const absolutePath = resolve(path);
      return {
        async sync() {
          events.push(`sync:${absolutePath}`);
          const failure = syncFailure?.(absolutePath);
          if (failure !== undefined) throw failure;
        },
        async close() {},
      };
    },
  };
}

function fakeOperations(
  events,
  {
    stagingSyncError,
    directoryOpenError,
    directoryOpenErrorAt,
    directorySyncError,
  } = {},
) {
  let directoryOpenCount = 0;
  return {
    async stat() {
      events.push("stat-directory");
      return { isDirectory: () => true };
    },
    async open(_path, mode) {
      if (mode === "wx") {
        events.push("open-staging");
        return {
          async writeFile() {
            events.push("write-staging");
          },
          async sync() {
            events.push("sync-staging");
            if (stagingSyncError !== undefined) throw stagingSyncError;
          },
          async close() {
            events.push("close-staging");
          },
        };
      }
      events.push("open-directory");
      directoryOpenCount += 1;
      if (
        directoryOpenError !== undefined
        && (
          directoryOpenErrorAt === undefined
          || directoryOpenCount === directoryOpenErrorAt
        )
      ) {
        throw directoryOpenError;
      }
      return {
        async sync() {
          events.push("sync-directory");
          if (directorySyncError !== undefined) throw directorySyncError;
        },
        async close() {
          events.push("close-directory");
        },
      };
    },
    async rename() {
      events.push("rename");
    },
    async rm() {
      events.push("remove-staging");
    },
  };
}
