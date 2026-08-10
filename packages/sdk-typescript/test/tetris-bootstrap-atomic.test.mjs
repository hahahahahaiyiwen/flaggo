import { createHash, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import {
  constants,
  access,
  lstat,
  mkdir,
  open,
  readFile,
  realpath,
  readdir,
  rename,
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";

import {
  createDirectoryDurable,
  publishJsonArtifact,
  publishJsonGeneration,
  resolveJsonGeneration,
  syncDirectory,
  writeJsonAtomic,
} from "../../../examples/tetris-integration/durable-json.mjs";
import {
  createWindowsDirectoryHelper,
  createWindowsDirectoryHelperServer,
  flushWindowsDirectory,
  validateWindowsPath,
} from "../../../examples/tetris-integration/windows-directory-helper.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const realOperations = {
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
  validateWindowsPath: process.platform === "win32"
    ? undefined
    : validateWindowsPath,
};

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

  it("uses the Windows helper instead of treating EISDIR as durable", async () => {
    const events = [];
    await syncDirectory("durable-directory", {
      platform: "win32",
      async flushDirectory(path) {
        events.push(resolve(path));
      },
      async open() {
        throw Object.assign(new Error("must not use Node directory open"), {
          code: "EISDIR",
        });
      },
    });

    expect(events).toEqual([resolve("durable-directory")]);
  });

  it("surfaces a Windows helper directory flush failure", async () => {
    const expected = new Error("FlushFileBuffers failed");

    await expect(syncDirectory("durable-directory", {
      platform: "win32",
      async flushDirectory() {
        throw expected;
      },
    })).rejects.toBe(expected);
  });

  it("fails closed when Unix cannot validate the opened descriptor", async () => {
    const events = [];
    const operations = {
      ...fakeOperations(events),
      platform: "darwin",
    };

    await expect(writeJsonAtomic(
      "state.json",
      { version: 1 },
      undefined,
      operations,
    )).rejects.toThrow(
      "opened-descriptor containment is supported only on Linux and Windows",
    );

    expect(events).toContain("close-staging");
    expect(events).toContain("remove-staging");
    expect(events).not.toContain("write-staging");
    expect(events).not.toContain("rename");
  });

  it.runIf(process.platform !== "win32")(
    "validates Windows native requests with a fake invocation",
    async () => {
      const requests = [];
      const helper = createWindowsDirectoryHelper({
        platform: "win32",
        async invoke(request) {
          requests.push(request);
          if (request.operation === "resolve-generation") {
            return {
              contents: {
                state: Buffer.from('{"version":1}\n').toString("base64"),
              },
            };
          }
          return { resolvedPath: request.path };
        },
      });
      const root = resolve(repositoryRoot, ".flaggo", "fake-helper-root");
      const path = resolve(root, "state.json");

      await helper.writeAtomic(root, path, { version: 1 });
      await helper.publishGeneration(root, { state: { version: 1 } });
      await helper.resolveGeneration(root, ["state"]);

      expect(requests.map(({ operation }) => operation)).toEqual([
        "write-atomic",
        "publish-generation",
        "resolve-generation",
      ]);
      expect(JSON.parse(Buffer.from(
        requests[0].contentBase64,
        "base64",
      ).toString("utf8"))).toEqual({ version: 1 });
      await expect(helper.writeAtomic(
        root,
        resolve(root, "..", "outside.json"),
        {},
      )).rejects.toThrow("escapes configured root");
      await expect(helper.publishGeneration(
        root,
        { "../outside": {} },
      )).rejects.toBeInstanceOf(TypeError);
      expect(requests).toHaveLength(3);
    },
  );

  it("shares one helper startup across 50 calls and lets Node exit", async () => {
    const helperModuleUrl = pathToFileURL(resolve(
      repositoryRoot,
      "examples/tetris-integration/windows-directory-helper.mjs",
    )).href;
    const clientSource = `
      import { spawn } from "node:child_process";
      import { createWindowsDirectoryHelperServer } from ${
        JSON.stringify(helperModuleUrl)
      };
      const helperSource = ${JSON.stringify(testHelperServerSource())};
      let spawnCount = 0;
      const controller = createWindowsDirectoryHelperServer({
        idleMilliseconds: 25,
        findExecutable: async () => ({
          command: process.execPath,
          args: ["--input-type=module", "--eval", helperSource, "--"],
        }),
        spawnProcess(command, args, options) {
          spawnCount += 1;
          return spawn(command, args, options);
        },
      });
      const results = await Promise.all(
        Array.from({ length: 50 }, (_, index) =>
          controller.invoke({ operation: "echo", index }))
      );
      if (results.some((result, index) => result.index !== index)) {
        throw new Error("Concurrent helper response mismatch.");
      }
      console.log(spawnCount);
    `;

    const result = await runNodeSource(clientSource);

    expect(result).toMatchObject({ code: 0, signal: null });
    expect(result.stderr).toBe("");
    expect(result.stdout.trim()).toBe("1");
  });

  it("shuts down the idle helper before starting a fresh child", async () => {
    const helperSource = testHelperServerSource();
    let spawnCount = 0;
    let closeCount = 0;
    const controller = createWindowsDirectoryHelperServer({
      manageProcessLifecycle: false,
      idleMilliseconds: 25,
      findExecutable: async () => ({
        command: process.execPath,
        args: ["--input-type=module", "--eval", helperSource, "--"],
      }),
      spawnProcess(command, args, options) {
        spawnCount += 1;
        const child = spawn(command, args, options);
        child.once("close", () => {
          closeCount += 1;
        });
        return child;
      },
    });
    try {
      const first = await controller.invoke({
        operation: "echo",
        index: 1,
        includePid: true,
      });
      await waitForCondition(() => closeCount === 1);

      const second = await controller.invoke({
        operation: "echo",
        index: 2,
        includePid: true,
      });

      expect(spawnCount).toBe(2);
      expect(second.pid).not.toBe(first.pid);
    } finally {
      await controller.shutdown();
    }
  });

  it("serialization failure does not pin the helper or pending request", async () => {
    const helperSource = testHelperServerSource();
    let closeCount = 0;
    const controller = createWindowsDirectoryHelperServer({
      manageProcessLifecycle: false,
      idleMilliseconds: 25,
      findExecutable: async () => ({
        command: process.execPath,
        args: ["--input-type=module", "--eval", helperSource, "--"],
      }),
      spawnProcess(command, args, options) {
        const child = spawn(command, args, options);
        child.once("close", () => {
          closeCount += 1;
        });
        return child;
      },
    });
    const request = { operation: "echo", index: 1 };
    request.circular = request;
    try {
      await expect(controller.invoke(request)).rejects.toBeInstanceOf(
        TypeError,
      );
      await waitForCondition(() => closeCount === 1);
    } finally {
      await controller.shutdown();
    }
  });

  it("test reset reaps the owned helper and permits reuse", async () => {
    const helperSource = testHelperServerSource();
    let spawnCount = 0;
    let closeCount = 0;
    const controller = createWindowsDirectoryHelperServer({
      manageProcessLifecycle: false,
      idleMilliseconds: 60_000,
      findExecutable: async () => ({
        command: process.execPath,
        args: ["--input-type=module", "--eval", helperSource, "--"],
      }),
      spawnProcess(command, args, options) {
        spawnCount += 1;
        const child = spawn(command, args, options);
        child.once("close", () => {
          closeCount += 1;
        });
        return child;
      },
    });
    try {
      const first = await controller.invoke({
        operation: "echo",
        index: 1,
        includePid: true,
      });
      await controller.reset();
      expect(closeCount).toBe(1);

      const second = await controller.invoke({
        operation: "echo",
        index: 2,
        includePid: true,
      });
      expect(spawnCount).toBe(2);
      expect(second.pid).not.toBe(first.pid);
    } finally {
      await controller.shutdown();
    }
  });

  it("process exit synchronously terminates its owned helper", async () => {
    const helperModuleUrl = pathToFileURL(resolve(
      repositoryRoot,
      "examples/tetris-integration/windows-directory-helper.mjs",
    )).href;
    const clientSource = `
      import { spawn } from "node:child_process";
      import { createWindowsDirectoryHelperServer } from ${
        JSON.stringify(helperModuleUrl)
      };
      const helperSource = ${JSON.stringify(testHelperServerSource())};
      const controller = createWindowsDirectoryHelperServer({
        idleMilliseconds: 60_000,
        findExecutable: async () => ({
          command: process.execPath,
          args: ["--input-type=module", "--eval", helperSource, "--"],
        }),
        spawnProcess: spawn,
      });
      const result = await controller.invoke({
        operation: "echo",
        index: 1,
        includePid: true,
      });
      process.stdout.write(String(result.pid) + "\\n", () => process.exit(0));
    `;

    const result = await runNodeSource(clientSource);

    expect(result).toMatchObject({ code: 0, signal: null });
    expect(result.stderr).toBe("");
    await waitForProcessExit(Number(result.stdout.trim()));
  });

  it("process exit during initialization terminates the starting child", async () => {
    const helperModuleUrl = pathToFileURL(resolve(
      repositoryRoot,
      "examples/tetris-integration/windows-directory-helper.mjs",
    )).href;
    const clientSource = `
      import { spawn } from "node:child_process";
      import { writeSync } from "node:fs";
      import { createWindowsDirectoryHelperServer } from ${
        JSON.stringify(helperModuleUrl)
      };
      const helperSource = ${JSON.stringify(testHelperServerSource())};
      const controller = createWindowsDirectoryHelperServer({
        idleMilliseconds: 60_000,
        findExecutable: async () => ({
          command: process.execPath,
          args: ["--input-type=module", "--eval", helperSource, "--"],
        }),
        spawnProcess(command, args, options) {
          const child = spawn(command, args, options);
          writeSync(1, String(child.pid) + "\\n");
          queueMicrotask(() => process.exit(0));
          return child;
        },
      });
      void controller.invoke({ operation: "echo", index: 1 });
    `;

    const result = await runNodeSource(clientSource);

    expect(result).toMatchObject({ code: 0, signal: null });
    expect(result.stderr).toBe("");
    await waitForProcessExit(Number(result.stdout.trim()));
  });

  it("reaps a shared failed startup and permits a fresh retry", async () => {
    const missingExecutable = artifactPath("missing-helper.exe");
    const helperSource = testHelperServerSource();
    let spawnCount = 0;
    let failedChildClosed = false;
    const controller = createWindowsDirectoryHelperServer({
      manageProcessLifecycle: false,
      idleMilliseconds: 60_000,
      findExecutable: async () => ({
        command: process.execPath,
        args: ["--input-type=module", "--eval", helperSource, "--"],
      }),
      spawnProcess(command, args, options) {
        spawnCount += 1;
        const child = spawnCount === 1
          ? spawn(missingExecutable, [], options)
          : spawn(command, args, options);
        if (spawnCount === 1) {
          child.once("close", () => {
            failedChildClosed = true;
          });
        }
        return child;
      },
    });
    try {
      const failures = await Promise.allSettled(
        Array.from({ length: 50 }, (_, index) =>
          controller.invoke({ operation: "echo", index })),
      );

      expect(failures.every(({ status }) => status === "rejected")).toBe(true);
      expect(spawnCount).toBe(1);
      expect(failedChildClosed).toBe(true);

      await expect(controller.invoke({ operation: "echo", index: 51 }))
        .resolves.toEqual({ index: 51 });
      expect(spawnCount).toBe(2);
    } finally {
      await controller.shutdown();
    }
  });

  it.runIf(process.platform === "win32")(
    "native helper rejects an ancestor junction without symlink privilege",
    async () => {
      const root = artifactPath("native-junction-root");
      const outside = artifactPath("native-junction-outside");
      await mkdir(root, { recursive: true });
      await mkdir(outside, { recursive: true });
      await createWindowsJunction(
        resolve(root, "generations"),
        outside,
      );
      try {
        await expect(publishJsonGeneration(
          root,
          generationFiles("unsafe"),
        )).rejects.toMatchObject({ code: "reparse_point" });
        expect(await readdir(outside)).toEqual([]);
        expect(await generationNames(root)).toEqual([]);
      } finally {
        await removeWindowsJunction(resolve(root, "generations"));
        await rm(root, { recursive: true, force: true });
        await rm(outside, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "native helper rejects a junction restored after its exact handle opens",
    async () => {
      const root = artifactPath("native-junction-swap-back");
      const outside = artifactPath("native-junction-swap-back-outside");
      const generations = resolve(root, "generations");
      const signalPath = `${root}-opened`;
      const releasePath = `${root}-release`;
      await mkdir(root, { recursive: true });
      await mkdir(outside, { recursive: true });
      await createWindowsJunction(generations, outside);
      const files = Object.fromEntries(
        Object.entries(generationFiles("unsafe")).map(([name, value]) => [
          name,
          Buffer.from(
            `${JSON.stringify(value, null, 2)}\n`,
            "utf8",
          ).toString("base64"),
        ]),
      );
      try {
        const response = await invokeActualHelperWithPause(
          {
            id: randomUUID(),
            operation: "publish-generation",
            root,
            files,
            testHook: {
              afterOpenPath: generations,
              signalPath,
              releasePath,
            },
          },
          signalPath,
          releasePath,
          async () => {
            await removeWindowsJunction(generations);
            await mkdir(generations);
          },
        );

        expect(response.ok).toBe(false);
        expect(response.error.code).toBe("reparse_point");
        expect(await readdir(outside)).toEqual([]);
        await expect(readFile(resolve(root, "current.json")))
          .rejects.toMatchObject({ code: "ENOENT" });
      } finally {
        await removeWindowsJunction(generations);
        await rm(root, { recursive: true, force: true });
        await rm(outside, { recursive: true, force: true });
        await rm(signalPath, { force: true });
        await rm(releasePath, { force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "native helper rejects a final pointer junction reparse",
    async () => {
      const root = artifactPath("native-final-junction");
      const outside = artifactPath("native-final-junction-outside");
      const pointer = resolve(root, "current.json");
      await mkdir(root, { recursive: true });
      await mkdir(outside, { recursive: true });
      await createWindowsJunction(pointer, outside);
      try {
        await expect(publishJsonGeneration(
          root,
          generationFiles("unsafe"),
        )).rejects.toMatchObject({ code: "reparse_point" });
        expect(await readdir(outside)).toEqual([]);
        expect(await generationNames(root)).toEqual([]);
      } finally {
        await removeWindowsJunction(pointer);
        await rm(root, { recursive: true, force: true });
        await rm(outside, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "native helper rejects a final artifact junction reparse",
    async () => {
      const root = artifactPath("native-final-artifact-junction");
      const outside = artifactPath(
        "native-final-artifact-junction-outside",
      );
      await mkdir(outside, { recursive: true });
      let artifactPathToReplace;
      try {
        const publication = await publishJsonGeneration(
          root,
          generationFiles("safe"),
        );
        artifactPathToReplace = publication.paths.state;
        await rm(artifactPathToReplace);
        await createWindowsJunction(artifactPathToReplace, outside);

        await expect(resolveJsonGeneration(
          root,
          ["receipt", "state", "evidence"],
        )).rejects.toMatchObject({ code: "reparse_point" });
        expect(await readdir(outside)).toEqual([]);
      } finally {
        if (artifactPathToReplace !== undefined) {
          await removeWindowsJunction(artifactPathToReplace);
        }
        await rm(root, { recursive: true, force: true });
        await rm(outside, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32").each([
    "after-native-rename",
    "after-rename-validation",
  ])(
    "retains committed native generation after failure %s",
    async (failAt) => {
      const root = artifactPath(`native-committed-${failAt}`);
      const controller = await createActualWindowsHelperController();
      try {
        const oldGeneration = await controller.invoke({
          operation: "publish-generation",
          root,
          files: encodedGenerationFiles("old"),
        });

        await expect(controller.invoke({
          operation: "publish-generation",
          root,
          files: encodedGenerationFiles("new"),
          testHook: { failAt },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        const manifest = JSON.parse(
          await readFile(resolve(root, "current.json"), "utf8"),
        );
        const resolved = await controller.invoke({
          operation: "resolve-generation",
          root,
          requiredNames: ["receipt", "state", "evidence"],
        });
        expect(manifest.generation).not.toBe(oldGeneration.generation);
        expect(resolved.generation).toBe(manifest.generation);
        expect(JSON.parse(Buffer.from(
          resolved.contents.state,
          "base64",
        ).toString("utf8"))).toEqual({ generation: "new" });
        expect(await generationNames(root)).toEqual([
          oldGeneration.generation,
          manifest.generation,
        ].sort());
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "cleans a native generation when failure precedes rename commit",
    async () => {
      const root = artifactPath("native-pre-rename-failure");
      const controller = await createActualWindowsHelperController();
      try {
        const oldGeneration = await controller.invoke({
          operation: "publish-generation",
          root,
          files: encodedGenerationFiles("old"),
        });

        await expect(controller.invoke({
          operation: "publish-generation",
          root,
          files: encodedGenerationFiles("new"),
          testHook: { failAt: "before-native-rename" },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        const manifest = JSON.parse(
          await readFile(resolve(root, "current.json"), "utf8"),
        );
        expect(manifest.generation).toBe(oldGeneration.generation);
        expect(await generationNames(root)).toEqual([
          oldGeneration.generation,
        ]);
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32").each([
    "after-native-rename",
    "after-rename-validation",
  ])(
    "retains a committed native atomic file after failure %s",
    async (failAt) => {
      const root = artifactPath(`native-atomic-${failAt}`);
      const path = resolve(root, "state.json");
      const controller = await createActualWindowsHelperController();
      try {
        await controller.invoke({
          operation: "write-atomic",
          root,
          path,
          contentBase64: encodedJson({ generation: "old" }),
        });

        await expect(controller.invoke({
          operation: "write-atomic",
          root,
          path,
          contentBase64: encodedJson({ generation: "new" }),
          testHook: { failAt },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        expect(JSON.parse(await readFile(path, "utf8")))
          .toEqual({ generation: "new" });
        expect(await readdir(root)).toEqual(["state.json"]);
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "cleans native atomic staging when failure precedes rename commit",
    async () => {
      const root = artifactPath("native-atomic-pre-rename");
      const path = resolve(root, "state.json");
      const controller = await createActualWindowsHelperController();
      try {
        await controller.invoke({
          operation: "write-atomic",
          root,
          path,
          contentBase64: encodedJson({ generation: "old" }),
        });

        await expect(controller.invoke({
          operation: "write-atomic",
          root,
          path,
          contentBase64: encodedJson({ generation: "new" }),
          testHook: { failAt: "before-native-rename" },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        expect(JSON.parse(await readFile(path, "utf8")))
          .toEqual({ generation: "old" });
        expect(await readdir(root)).toEqual(["state.json"]);
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32").each([
    "after-native-rename",
    "after-rename-validation",
  ])(
    "retains a committed native artifact after failure %s",
    async (failAt) => {
      const root = artifactPath(`native-artifact-${failAt}`);
      const path = resolve(root, "state.commit.json");
      const controller = await createActualWindowsHelperController();
      try {
        await controller.invoke({
          operation: "publish-artifact",
          root,
          path,
          contentBase64: encodedJson({ generation: "old" }),
        });

        await expect(controller.invoke({
          operation: "publish-artifact",
          root,
          path,
          contentBase64: encodedJson({ generation: "new" }),
          testHook: { failAt },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        expect(await resolveDirectJson(path))
          .toEqual({ generation: "new" });
        expect((await readdir(root)).some((name) => name.endsWith(".tmp")))
          .toBe(false);
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "cleans native artifact staging when failure precedes rename commit",
    async () => {
      const root = artifactPath("native-artifact-pre-rename");
      const path = resolve(root, "state.commit.json");
      const controller = await createActualWindowsHelperController();
      try {
        const oldArtifact = await controller.invoke({
          operation: "publish-artifact",
          root,
          path,
          contentBase64: encodedJson({ generation: "old" }),
        });

        await expect(controller.invoke({
          operation: "publish-artifact",
          root,
          path,
          contentBase64: encodedJson({ generation: "new" }),
          testHook: { failAt: "before-native-rename" },
        })).rejects.toMatchObject({ code: "test_hook_failure" });

        expect(await resolveDirectJson(path))
          .toEqual({ generation: "old" });
        expect((await readdir(root)).sort()).toEqual([
          basename(oldArtifact.artifactPath),
          "state.commit.json",
        ].sort());
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
  );

  it.runIf(process.platform === "win32")(
    "keeps handles bounded across 1000 invalid traversals in one server",
    async () => {
      const root = artifactPath("native-invalid-traversal");
      const traversed = resolve(root, "one", "two");
      const missing = resolve(traversed, "missing");
      await mkdir(traversed, { recursive: true });
      let spawnCount = 0;
      const controller = await createActualWindowsHelperController({
        onSpawn() {
          spawnCount += 1;
        },
      });
      try {
        for (let index = 0; index < 10; index += 1) {
          await expect(controller.invoke({
            operation: "validate",
            path: missing,
          })).rejects.toMatchObject({ code: "not_found" });
        }
        const baseline = await controller.invoke({
          operation: "test-handle-count",
        });

        for (let index = 0; index < 1_000; index += 1) {
          await expect(controller.invoke({
            operation: "validate",
            path: missing,
          })).rejects.toMatchObject({ code: "not_found" });
        }

        const after = await controller.invoke({
          operation: "test-handle-count",
        });
        expect(spawnCount).toBe(1);
        expect(after.handleCount).toBeLessThanOrEqual(
          baseline.handleCount + 16,
        );
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
    },
    30_000,
  );

  it.runIf(process.platform === "win32")(
    "closes constructor ownership after failure at every opened component",
    async () => {
      const root = artifactPath("native-component-failure");
      const target = resolve(root, "one", "two");
      await mkdir(target, { recursive: true });
      const controller = await createActualWindowsHelperController();
      try {
        const baseline = await controller.invoke({
          operation: "test-handle-count",
        });
        for (const componentPath of absoluteAncestorPaths(target)) {
          await expect(controller.invoke({
            operation: "validate",
            path: target,
            testHook: {
              afterOpenPath: componentPath,
              failAt: "after-open",
            },
          })).rejects.toMatchObject({ code: "test_hook_failure" });
        }
        const after = await controller.invoke({
          operation: "test-handle-count",
        });
        expect(after.handleCount).toBeLessThanOrEqual(
          baseline.handleCount + 16,
        );
      } finally {
        await controller.shutdown();
        await rm(root, { recursive: true, force: true });
      }
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
      platform: undefined,
      createFileFlags: "wx",
      readFileFlags: "r",
      async open(path, mode) {
        const handle = await open(path, mode);
        if (mode !== "r") return handle;
        directoryOpenCount += 1;
        return {
          async sync() {
            if (directoryOpenCount === 3) {
              throw new Error("injected parent sync failure");
            }
            return syncDirectoryHandle(handle);
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

  it("rejects a publication root symbolic link or junction", async (context) => {
    const target = artifactPath("bootstrap-link-root-target");
    const linkedRoot = artifactPath("bootstrap-link-root");
    await mkdir(target, { recursive: true });
    await createDirectoryLinkOrSkip(context, linkedRoot, target);
    try {
      await expect(publishJsonGeneration(
        linkedRoot,
        generationFiles("linked"),
      )).rejects.toThrow(/symbolic link|reparse point/iu);
    } finally {
      await rm(linkedRoot, { recursive: true, force: true });
      await rm(target, { recursive: true, force: true });
    }
  });

  it("rejects a generations component symbolic link or junction", async (context) => {
    const root = artifactPath("bootstrap-link-component");
    const outside = artifactPath("bootstrap-link-component-target");
    await mkdir(root, { recursive: true });
    await mkdir(outside, { recursive: true });
    await createDirectoryLinkOrSkip(
      context,
      resolve(root, "generations"),
      outside,
    );
    try {
      await expect(publishJsonGeneration(
        root,
        generationFiles("linked"),
      )).rejects.toThrow(/symbolic link|reparse point/iu);
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("rejects a symbolic-link generation artifact during resolution", async (context) => {
    const root = artifactPath("bootstrap-link-artifact");
    const outside = artifactPath("bootstrap-link-artifact-target");
    try {
      const publication = await publishJsonGeneration(
        root,
        generationFiles("linked"),
      );
      await writeFile(outside, await readFile(publication.paths.state));
      await rm(publication.paths.state);
      await createFileLinkOrSkip(context, publication.paths.state, outside);

      await expect(resolveJsonGeneration(
        root,
        ["receipt", "state", "evidence"],
      )).rejects.toThrow(/symbolic link|reparse point/iu);
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(outside, { force: true });
    }
  });

  it("rejects a symbolic-link current pointer before publication", async (context) => {
    const root = artifactPath("bootstrap-link-pointer");
    const outside = artifactPath("bootstrap-link-pointer-target");
    try {
      await publishJsonGeneration(root, generationFiles("old"));
      await writeFile(outside, "{}\n");
      await rm(resolve(root, "current.json"));
      await createFileLinkOrSkip(
        context,
        resolve(root, "current.json"),
        outside,
      );

      await expect(publishJsonGeneration(
        root,
        generationFiles("new"),
      )).rejects.toThrow(/symbolic link|reparse point/iu);
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(outside, { force: true });
    }
  });

  it("fails closed when the publication directory is swapped before open", async (context) => {
    const root = artifactPath("direct-swap-root");
    const original = `${root}-original`;
    const outside = artifactPath("direct-swap-outside");
    await mkdir(root, { recursive: true });
    await mkdir(outside, { recursive: true });
    let swapped = false;
    const operations = {
      ...realOperations,
      async open(path, mode) {
        if (
          !swapped
          && dirname(String(path)) === root
          && basename(String(path)).endsWith(".json")
          && !basename(String(path)).includes(".commit.")
        ) {
          swapped = true;
          await rename(root, original);
          await createDirectoryLinkOrSkip(context, root, outside);
        }
        return open(path, mode);
      },
    };
    try {
      await expect(publishJsonArtifact(
        resolve(root, "state.commit.json"),
        { generation: "unsafe" },
        undefined,
        operations,
      )).rejects.toThrow(
        /symbolic link|reparse point|configured root|non-link directory/iu,
      );
      expect(await readdir(outside)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(original, { recursive: true, force: true });
      await rm(outside, { recursive: true, force: true });
    }
  });

  it("fails closed when the publication directory is replaced normally", async () => {
    const root = artifactPath("direct-regular-swap-root");
    const original = `${root}-original`;
    await mkdir(root, { recursive: true });
    let swapped = false;
    const operations = {
      ...realOperations,
      async open(path, mode) {
        if (
          !swapped
          && dirname(String(path)) === root
          && basename(String(path)).endsWith(".json")
          && !basename(String(path)).includes(".commit.")
        ) {
          swapped = true;
          await rename(root, original);
          await mkdir(root);
        }
        return open(path, mode);
      },
    };
    try {
      await expect(publishJsonArtifact(
        resolve(root, "state.commit.json"),
        { generation: "unsafe" },
        undefined,
        operations,
      )).rejects.toThrow("changed directory identity");
      expect(await readdir(root)).toEqual([]);
      expect(await readdir(original)).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
      await rm(original, { recursive: true, force: true });
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

function testHelperServerSource() {
  return `
    process.stdin.setEncoding("utf8");
    let buffered = "";
    process.stdin.on("data", (chunk) => {
      buffered += chunk;
      let newline;
      while ((newline = buffered.indexOf("\\n")) >= 0) {
        const line = buffered.slice(0, newline);
        buffered = buffered.slice(newline + 1);
        if (line.length === 0) continue;
        const request = JSON.parse(line);
        process.stdout.write(JSON.stringify({
          id: request.id,
          ok: true,
          result: {
            index: request.index,
            ...(request.includePid === true ? { pid: process.pid } : {}),
          },
        }) + "\\n");
      }
    });
  `;
}

async function runNodeSource(source) {
  const child = spawn(
    process.execPath,
    ["--input-type=module", "--eval", source],
    {
      cwd: repositoryRoot,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  return new Promise((resolvePromise, rejectPromise) => {
    const timer = setTimeout(() => {
      child.kill();
      rejectPromise(new Error("Timed out waiting for test Node process exit."));
    }, 10_000);
    child.once("error", (error) => {
      clearTimeout(timer);
      rejectPromise(error);
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      resolvePromise({ code, signal, stdout, stderr });
    });
  });
}

async function waitForCondition(condition) {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    if (condition()) return;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 10));
  }
  throw new Error("Timed out waiting for helper lifecycle condition.");
}

async function waitForProcessExit(pid) {
  await waitForCondition(() => {
    try {
      process.kill(pid, 0);
      return false;
    } catch (error) {
      if (error?.code === "ESRCH") return true;
      throw error;
    }
  });
}

async function createActualWindowsHelperController({ onSpawn } = {}) {
  const executable = await findBuiltHelper();
  return createWindowsDirectoryHelperServer({
    manageProcessLifecycle: false,
    idleMilliseconds: 60_000,
    environment: {
      ...process.env,
      FLAGGO_DIRECTORY_HELPER_TEST_HOOKS: "1",
    },
    findExecutable: async () => ({ command: executable, args: [] }),
    spawnProcess(command, args, options) {
      onSpawn?.();
      return spawn(command, args, options);
    },
  });
}

function encodedGenerationFiles(generation) {
  return Object.fromEntries(
    Object.entries(generationFiles(generation)).map(([name, value]) => [
      name,
      Buffer.from(
        `${JSON.stringify(value, null, 2)}\n`,
        "utf8",
      ).toString("base64"),
    ]),
  );
}

function encodedJson(value) {
  return Buffer.from(
    `${JSON.stringify(value, null, 2)}\n`,
    "utf8",
  ).toString("base64");
}

function absoluteAncestorPaths(path) {
  const paths = [];
  let current = resolve(path);
  while (true) {
    paths.unshift(current);
    const parent = dirname(current);
    if (parent === current) return paths;
    current = parent;
  }
}

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
    platform: undefined,
    createFileFlags: "wx",
    readFileFlags: "r",
    async open(path, mode) {
      const handle = await open(path, mode);
      if (mode !== "r") return handle;
      directorySyncCount += 1;
      const syncEvent = directorySyncCount === 3
        ? "artifact-directory-sync"
        : directorySyncCount === 6
          ? "descriptor-directory-sync"
          : undefined;
      return {
        async sync() {
          if (syncEvent === undefined) {
            return syncDirectoryHandle(handle);
          }
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
          return syncDirectoryHandle(handle);
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
    platform: undefined,
    createFileFlags: "wx",
    readFileFlags: "r",
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
            if (creationSync) return syncDirectoryHandle(handle);
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
              if (rootStage === undefined) {
                return syncDirectoryHandle(handle);
              }
              events.push(rootStage);
              if (stage === rootStage) {
                throw new Error(`injected ${rootStage} failure`);
              }
            }
            return syncDirectoryHandle(handle);
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

async function syncDirectoryHandle(handle) {
  try {
    return await handle.sync();
  } catch (error) {
    if (
      process.platform === "win32"
      && ["EISDIR", "EBADF", "EINVAL", "EPERM"].includes(error?.code)
    ) {
      return;
    }
    throw error;
  }
}

async function createDirectoryLinkOrSkip(context, path, target) {
  try {
    await symlink(
      target,
      path,
      process.platform === "win32" ? "junction" : "dir",
    );
  } catch (error) {
    if (
      process.platform === "win32"
      && ["EPERM", "EACCES", "UNKNOWN"].includes(error?.code)
    ) {
      context.skip();
      return;
    }
    throw error;
  }
}

async function createWindowsJunction(path, target) {
  await symlink(target, path, "junction");
  expect((await lstat(path)).isSymbolicLink()).toBe(true);
}

async function removeWindowsJunction(path) {
  try {
    if ((await lstat(path)).isSymbolicLink()) {
      await rm(path, { force: true });
    }
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

async function invokeActualHelperWithPause(
  request,
  signalPath,
  releasePath,
  onPause,
) {
  const executable = await findBuiltHelper();
  const child = spawn(executable, ["--server"], {
    cwd: repositoryRoot,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
    env: {
      ...process.env,
      FLAGGO_DIRECTORY_HELPER_TEST_HOOKS: "1",
    },
  });
  child.stderr.setEncoding("utf8");
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  let exited = false;
  const exit = new Promise((resolvePromise) => {
    child.once("exit", (code) => {
      exited = true;
      resolvePromise(code);
    });
  });
  const response = new Promise((resolvePromise, rejectPromise) => {
    child.stdout.setEncoding("utf8");
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const newline = output.indexOf("\n");
      if (newline >= 0) {
        try {
          resolvePromise(JSON.parse(output.slice(0, newline)));
        } catch (error) {
          rejectPromise(error);
        }
      }
    });
    child.once("error", rejectPromise);
    child.once("exit", (code) => {
      if (code !== 0 && output.length === 0) {
        rejectPromise(new Error(
          `Windows helper exited with ${code}: ${stderr}`,
        ));
      }
    });
  });
  child.stdin.write(`${JSON.stringify(request)}\n`);
  try {
    await waitForPath(signalPath);
    await onPause();
    await writeFile(releasePath, "release");
    const result = await response;
    await stopActualHelper(child, exit, () => exited);
    return result;
  } finally {
    await stopActualHelper(child, exit, () => exited);
  }
}

async function stopActualHelper(child, exit, hasExited) {
  if (hasExited()) return;
  if (!child.stdin.destroyed) child.stdin.end();
  await Promise.race([
    exit,
    new Promise((resolvePromise) => setTimeout(resolvePromise, 250)),
  ]);
  if (
    !hasExited()
    && child.pid !== undefined
    && child.exitCode === null
    && child.signalCode === null
  ) {
    child.kill();
  }
  await exit;
}

async function findBuiltHelper() {
  const candidates = [
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
  ];
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
    }
  }
  throw new Error("The Windows helper must be built before this test.");
}

async function waitForPath(path) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      await access(path);
      return;
    } catch {
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 10));
    }
  }
  throw new Error(`Timed out waiting for '${path}'.`);
}

async function createFileLinkOrSkip(context, path, target) {
  try {
    await symlink(target, path, "file");
  } catch (error) {
    if (
      process.platform === "win32"
      && ["EPERM", "EACCES", "UNKNOWN"].includes(error?.code)
    ) {
      context.skip();
      return;
    }
    throw error;
  }
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
