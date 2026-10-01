import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";

import {
  createRustHostEnvironment,
  parseListeningUrl,
  rustBinaryPath,
  sqliteDatabaseUrl,
} from "../host-process.mjs";

test("listening observer accepts .NET and Rust host events", () => {
  assert.equal(
    parseListeningUrl(JSON.stringify({
      Category: "Microsoft.Hosting.Lifetime",
      State: { address: "http://127.0.0.1:5010" },
    })),
    "http://127.0.0.1:5010",
  );
  assert.equal(
    parseListeningUrl(JSON.stringify({
      event: "server.listening",
      address: "http://127.0.0.1:5011",
    })),
    "http://127.0.0.1:5011",
  );
  assert.equal(
    parseListeningUrl(JSON.stringify({
      event: "server.listening",
      address: "http://0.0.0.0:5011",
    })),
    undefined,
  );
});

test("Rust binary paths are platform-specific", () => {
  const root = resolve("repository");

  assert.equal(
    rustBinaryPath(root, "flaggo-otel-ingestion", {
      platform: "win32",
    }),
    resolve(root, "target", "debug", "flaggo-otel-ingestion.exe"),
  );
  assert.equal(
    rustBinaryPath(root, "flaggo-otel-ingestion", {
      platform: "linux",
      profile: "release",
    }),
    resolve(root, "target", "release", "flaggo-otel-ingestion"),
  );
});

test("Rust host environment requests an ephemeral loopback port", () => {
  const environment = createRustHostEnvironment(
    { FLAGGO_DATABASE_URL: "sqlite://flaggo.db" },
    { PATH: "test-path" },
  );

  assert.deepEqual(environment, {
    PATH: "test-path",
    FLAGGO_DATABASE_URL: "sqlite://flaggo.db",
    FLAGGO_OTEL_INGESTION_LISTEN_ADDRESS: "127.0.0.1:0",
  });
});

test("SQLite URLs preserve absolute local paths", () => {
  const databasePath = resolve("repository", "run state", "flaggo.db");
  const databaseUrl = sqliteDatabaseUrl(databasePath);

  assert.match(databaseUrl, /^sqlite:(?:[A-Za-z]:\/|\/)/u);
  assert.ok(databaseUrl.endsWith("/repository/run%20state/flaggo.db"));
});
