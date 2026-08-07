import { describe, expect, it } from "vitest";
import { EventEmitter } from "node:events";

import {
  createHostEnvironment,
  createHostLifecycle,
  createStructuredLogObserver,
  installSignalHandlers,
  parseListeningUrl,
  runWithCleanup,
  startControlAndDataHosts,
  startHungEndpoint,
  startUnavailableEndpoint,
  waitForReady,
} from "../../../examples/tetris-integration/host-process.mjs";

function listeningLine(address) {
  return JSON.stringify({
    EventId: 14,
    LogLevel: "Information",
    Category: "Microsoft.Hosting.Lifetime",
    Message: `Now listening on: ${address}`,
    State: {
      Message: `Now listening on: ${address}`,
      address,
    },
  });
}

describe("Tetris integration host discovery", () => {
  it("discovers a dynamically assigned loopback port from structured output", () => {
    expect(parseListeningUrl(listeningLine("http://127.0.0.1:43127"))).toBe(
      "http://127.0.0.1:43127",
    );
  });

  it("buffers split log chunks and ignores unrelated output", () => {
    const discovered = [];
    const observer = createStructuredLogObserver((url) => discovered.push(url));
    const line = listeningLine("http://127.0.0.1:51903");

    observer.write(`not json\n${line.slice(0, 37)}`);
    observer.write(`${line.slice(37)}\n`);
    observer.end();

    expect(discovered).toEqual(["http://127.0.0.1:51903"]);
  });

  it.each([
    "http://127.0.0.1:0",
    "http://localhost:43127",
    "https://127.0.0.1:43127",
  ])("rejects an unexpected listening address: %s", (address) => {
    expect(parseListeningUrl(listeningLine(address))).toBeUndefined();
  });

  it("keeps the dynamically allocated unavailable endpoint bound", async () => {
    const endpoint = await startUnavailableEndpoint();
    try {
      const url = new URL(endpoint.url);
      expect(url.hostname).toBe("127.0.0.1");
      expect(Number(url.port)).toBeGreaterThan(0);
      const response = await fetch(endpoint.url);
      expect(response.status).toBe(503);
      await expect(response.json()).resolves.toMatchObject({
        code: "service-unavailable",
        clientFallback: { eligible: true },
      });
      expect(endpoint.exited).toBe(false);
    } finally {
      await endpoint.stop();
    }
    expect(endpoint.exited).toBe(true);
  });

  it("forces both ASP.NET environment variables to Development", () => {
    const environment = createHostEnvironment(
      {
        ASPNETCORE_ENVIRONMENT: "Staging",
        DOTNET_ENVIRONMENT: "Test",
      },
      {
        ASPNETCORE_ENVIRONMENT: "Production",
        DOTNET_ENVIRONMENT: "Production",
      },
    );

    expect(environment.ASPNETCORE_ENVIRONMENT).toBe("Development");
    expect(environment.DOTNET_ENVIRONMENT).toBe("Development");
  });
});

describe("Tetris integration host lifecycle", () => {
  it("aborts during bootstrap before creating a data-plane host", async () => {
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    const processTarget = new EventEmitter();
    processTarget.exitCode = undefined;
    const uninstall = installSignalHandlers(lifecycle, processTarget);
    let dataHostCreated = false;

    try {
      const workflow = startControlAndDataHosts({
        lifecycle,
        createControlHost: () => host("control-plane"),
        waitForControlReady: async () => {},
        bootstrap: async () => {
          processTarget.emit("SIGINT");
          processTarget.emit("SIGTERM");
          return {};
        },
        createDataHost: () => {
          dataHostCreated = true;
          return host("data-plane");
        },
        waitForDataReady: async () => {},
      });

      await expect(workflow).rejects.toMatchObject({
        name: "AbortError",
        signal: "SIGINT",
        exitCode: 130,
      });
      expect(processTarget.exitCode).toBe(130);
      expect(dataHostCreated).toBe(false);
      await lifecycle.cleanup();
    } finally {
      uninstall();
    }
  });

  it("immediately stops a host presented after cleanup begins", async () => {
    let releaseFirst;
    let firstStopStarted;
    const firstStarted = new Promise((resolvePromise) => {
      firstStopStarted = resolvePromise;
    });
    const stopped = [];
    let removeCount = 0;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removeCount += 1;
      },
    });
    lifecycle.trackHost(host("first", async () => {
      firstStopStarted();
      await new Promise((resolvePromise) => {
        releaseFirst = resolvePromise;
      });
      stopped.push("first");
    }));

    const cleanup = lifecycle.cleanup();
    await firstStarted;
    expect(() => lifecycle.trackHost(host("late", async () => {
      stopped.push("late");
    }))).toThrow("cleanup began");
    releaseFirst();
    await cleanup;

    expect(stopped.sort()).toEqual(["first", "late"]);
    expect(removeCount).toBe(1);
  });

  it("waits for a deferred factory and stops the host resolved during cleanup", async () => {
    const factory = deferred();
    let stopCount = 0;
    let removeCount = 0;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removeCount += 1;
      },
    });
    const creation = lifecycle.startHostAsync(() => factory.promise);

    const cleanup = lifecycle.cleanup();
    factory.resolve(host("deferred", async () => {
      stopCount += 1;
    }));

    await expect(creation).rejects.toThrow(
      "host creation completed after cleanup began",
    );
    await cleanup;
    expect(stopCount).toBe(1);
    expect(removeCount).toBe(1);
  });

  it("includes a deferred factory rejection in cleanup failures", async () => {
    const factory = deferred();
    let removeCount = 0;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removeCount += 1;
      },
    });
    const creation = lifecycle.startHostAsync(() => factory.promise);

    const cleanup = lifecycle.cleanup();
    const factoryError = new Error("factory failed");
    factory.reject(factoryError);

    await expect(creation).rejects.toBe(factoryError);
    await expect(cleanup).rejects.toMatchObject({
      errors: [
        expect.objectContaining({
          message: "Failed to create an integration host.",
          cause: factoryError,
        }),
      ],
    });
    expect(removeCount).toBe(1);
  });

  it("shares repeated cleanup while a deferred factory is unresolved", async () => {
    const factory = deferred();
    let stopCount = 0;
    let removeCount = 0;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removeCount += 1;
      },
    });
    const creation = lifecycle.startHostAsync(() => factory.promise);

    const cleanups = [
      lifecycle.cleanup(),
      lifecycle.cleanup(),
      lifecycle.cleanup(),
    ];
    factory.resolve(host("deferred", async () => {
      stopCount += 1;
    }));

    await expect(creation).rejects.toThrow(
      "host creation completed after cleanup began",
    );
    await Promise.all(cleanups);
    await lifecycle.cleanup();
    expect(stopCount).toBe(1);
    expect(removeCount).toBe(1);
  });

  it("aggregates a stop failure from a host resolved during cleanup", async () => {
    const factory = deferred();
    const stopError = new Error("late stop failed");
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    const creation = lifecycle.startHostAsync(() => factory.promise);

    const cleanup = lifecycle.cleanup();
    factory.resolve(host("late-failing", async () => {
      throw stopError;
    }));

    await expect(creation).rejects.toThrow(
      "host creation completed after cleanup began",
    );
    await expect(cleanup).rejects.toMatchObject({
      errors: [
        expect.objectContaining({
          message: "Failed to stop integration host 'late-failing'.",
          cause: stopError,
        }),
      ],
    });
  });

  it("stops every host and removes the run directory when one stop rejects", async () => {
    const stopped = [];
    let removed = false;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removed = true;
      },
    });
    lifecycle.trackHost(host("failing", async () => {
      stopped.push("failing");
      throw new Error("stop failed");
    }));
    lifecycle.trackHost(host("healthy", async () => {
      stopped.push("healthy");
    }));

    await expect(lifecycle.cleanup()).rejects.toBeInstanceOf(AggregateError);
    expect(stopped.sort()).toEqual(["failing", "healthy"]);
    expect(removed).toBe(true);
  });

  it("makes repeated cleanup calls idempotent", async () => {
    let stopCount = 0;
    let removeCount = 0;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        removeCount += 1;
      },
    });
    lifecycle.trackHost(host("single", async () => {
      stopCount += 1;
    }));

    await Promise.all([
      lifecycle.cleanup(),
      lifecycle.cleanup(),
      lifecycle.cleanup(),
    ]);
    await lifecycle.cleanup();

    expect(stopCount).toBe(1);
    expect(removeCount).toBe(1);
  });

  it("retries a Windows-like open-log removal race after hosts drain", async () => {
    const stopped = [];
    let removalAttempts = 0;
    const lifecycle = createHostLifecycle({
      removeAttempts: 2,
      removeRetryDelayMilliseconds: 0,
      removeRunDirectory: async () => {
        removalAttempts += 1;
        if (removalAttempts === 1) {
          const error = new Error("file is in use");
          error.code = "EBUSY";
          throw error;
        }
        expect(stopped).toEqual(["data-plane"]);
      },
    });
    lifecycle.trackHost(host("data-plane", async () => {
      stopped.push("data-plane");
    }));

    await lifecycle.cleanup();

    expect(removalAttempts).toBe(2);
    expect(stopped).toEqual(["data-plane"]);
  });

  it("reports aggregate cleanup failure without replacing workflow failure", async () => {
    const original = new Error("workflow failed");
    let reported;
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {
        throw new Error("remove failed");
      },
    });

    lifecycle.trackHost(host("failing", async () => {
      throw new Error("stop failed");
    }));

    await expect(runWithCleanup(
      async () => {
        throw original;
      },
      lifecycle,
      (cleanupError) => {
        reported = cleanupError;
      },
    )).rejects.toBe(original);
    expect(reported).toBeInstanceOf(AggregateError);
    expect(reported.errors).toHaveLength(2);
  });

  it.each([0, 17])(
    "fails a passing workflow when a host exits unexpectedly with code %s",
    async (exitCode) => {
      const lifecycle = createHostLifecycle({
        removeRunDirectory: async () => {},
      });
      const crashed = crashableHost("crashed");
      lifecycle.trackHost(crashed);
      crashed.crash(exitCode);

      await expect(runWithCleanup(
        async () => "workflow-passed",
        lifecycle,
      )).rejects.toMatchObject({
        errors: [
          expect.objectContaining({
            message: expect.stringContaining("before shutdown was requested"),
            cause: expect.objectContaining({
              message: expect.stringContaining(`code ${exitCode}`),
            }),
          }),
        ],
      });
      expect(crashed.stopCount).toBe(1);
    },
  );

  it("treats requested host shutdown as successful", async () => {
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    const managed = crashableHost("requested-stop");
    lifecycle.trackHost(managed);

    await expect(runWithCleanup(
      async () => "passed",
      lifecycle,
    )).resolves.toBe("passed");
    expect(managed.stopCount).toBe(1);
  });

  it("detects a control-plane crash immediately after bootstrap", async () => {
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    const control = crashableHost("control-plane");
    let dataHostCreated = false;

    await expect(startControlAndDataHosts({
      lifecycle,
      createControlHost: () => control,
      waitForControlReady: async () => {},
      bootstrap: async () => {
        control.crash(0);
        return {};
      },
      createDataHost: () => {
        dataHostCreated = true;
        return host("data-plane");
      },
      waitForDataReady: async () => {},
    })).rejects.toThrow("exited unexpectedly");
    expect(dataHostCreated).toBe(false);
    await expect(lifecycle.cleanup()).rejects.toMatchObject({
      errors: [
        expect.objectContaining({
          message: expect.stringContaining("before shutdown was requested"),
        }),
      ],
    });
  });
});

describe("Tetris integration readiness deadlines", () => {
  it("aborts a never-responding probe at the overall deadline and drains it", async () => {
    const endpoint = await startHungEndpoint();
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    lifecycle.trackHost(endpoint);
    const started = Date.now();

    await expect(waitForReady(
      (signal) => fetch(endpoint.url, { signal }).then((response) => response.ok),
      endpoint,
      lifecycle.signal,
      {
        timeoutMilliseconds: 150,
        retryDelayMilliseconds: 10,
      },
    )).rejects.toThrow("did not become ready");

    expect(Date.now() - started).toBeLessThan(1000);
    await lifecycle.cleanup();
    expect(endpoint.exited).toBe(true);
    expect(endpoint.connectionCount).toBe(0);
  });

  it("retains a dependency diagnostic when the final probe times out", async () => {
    const lifecycle = createHostLifecycle({
      removeRunDirectory: async () => {},
    });
    let attempts = 0;
    const started = Date.now();
    let readinessError;

    try {
      await waitForReady(
        async (signal) => {
          attempts++;
          if (attempts === 1) {
            throw new Error(
              'HTTP 503: {"status":"not-ready","dependencies":{"audit":"unavailable"}}',
            );
          }

          await new Promise((_resolvePromise, reject) => {
            signal.addEventListener(
              "abort",
              () => reject(signal.reason),
              { once: true },
            );
          });
        },
        host("data-plane"),
        lifecycle.signal,
        {
          timeoutMilliseconds: 80,
          retryDelayMilliseconds: 5,
        },
      );
    } catch (error) {
      readinessError = error;
    }

    expect(readinessError).toBeInstanceOf(Error);
    expect(readinessError.message).toContain('"audit":"unavailable"');
    expect(readinessError.message).toContain(
      "readiness probe exceeded its remaining deadline",
    );
    expect(readinessError.message).toContain("within 80ms");
    expect(attempts).toBe(2);
    expect(Date.now() - started).toBeLessThan(1000);
    await lifecycle.cleanup();
  });
});

function host(name, stop = async () => {}) {
  return {
    name,
    exited: false,
    error: undefined,
    waitForListening: async () => "http://127.0.0.1:43127",
    stop,
  };
}

function crashableHost(name) {
  let unexpectedExit;
  let exited = false;
  let stopCount = 0;
  return {
    name,
    get exited() {
      return exited;
    },
    get error() {
      return undefined;
    },
    get unexpectedExit() {
      return unexpectedExit;
    },
    get stopCount() {
      return stopCount;
    },
    crash(exitCode) {
      exited = true;
      unexpectedExit = new Error(
        `${name} exited unexpectedly (code ${exitCode}, signal none).`,
      );
    },
    waitForListening: async () => "http://127.0.0.1:43127",
    async stop() {
      stopCount += 1;
    },
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
