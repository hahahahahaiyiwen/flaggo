import { randomUUID } from "node:crypto";
import {
  mkdir,
  open,
  readFile,
  readdir,
  rename,
  rm,
} from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import {
  publishJsonGeneration,
  resolveJsonGeneration,
  writeJsonAtomic,
} from "../../../examples/tetris-integration/durable-json.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const realOperations = { mkdir, open, readFile, rename, rm };

describe("Tetris durable atomic JSON writes", () => {
  it("syncs and closes staging before rename, then syncs the directory", async () => {
    const events = [];
    const operations = fakeOperations(events);

    await writeJsonAtomic("state.json", { version: 1 }, undefined, operations);

    expect(events).toEqual([
      "mkdir",
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
      "mkdir",
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
    "generations-mkdir",
    "generation-mkdir",
    "file-open",
    "file-write",
    "file-sync",
    "generation-sync",
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
        failingGenerationOperations(root, "root-sync", []),
      )).rejects.toThrow("injected root-sync failure");

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

function failingGenerationOperations(
  root,
  stage,
  events,
  { delayedSibling = false } = {},
) {
  let generationFileCount = 0;
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
      return mkdir(path, options);
    },
    async open(path, mode) {
      const pathText = String(path);
      const manifestStaging = basename(pathText).startsWith("current.json.");
      const generationDirectory = mode === "r"
        && basename(dirname(pathText)) === "generations";
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
            if (stage === "generation-sync" && generationDirectory) {
              throw new Error("injected generation-sync failure");
            }
            if (stage === "root-sync" && rootDirectory) {
              throw new Error("injected root-sync failure");
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
      if (
        stage === "manifest-rename"
        && basename(destination) === "current.json"
      ) {
        throw new Error("injected manifest-rename failure");
      }
      return rename(source, destination);
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

function fakeOperations(
  events,
  {
    stagingSyncError,
    directorySyncError,
  } = {},
) {
  return {
    async mkdir() {
      events.push("mkdir");
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
