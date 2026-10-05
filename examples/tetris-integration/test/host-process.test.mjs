import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";

import {
  createRustHostEnvironment,
  createStructuredLogObserver,
  parseMaterializationHealth,
  parseListeningUrl,
  rustBinaryPath,
  sqliteDatabaseUrl,
} from "../host-process.mjs";

test("listening observer accepts .NET and Rust service telemetry", () => {
  assert.equal(
    parseListeningUrl(JSON.stringify({
      Category: "Microsoft.Hosting.Lifetime",
      State: { address: "http://127.0.0.1:5010" },
    })),
    "http://127.0.0.1:5010",
  );
  assert.equal(
    parseListeningUrl(JSON.stringify({
      "event.name": "flaggo.service.started",
      "server.address": "http://127.0.0.1:5011",
    })),
    "http://127.0.0.1:5011",
  );
  assert.equal(
    parseListeningUrl(JSON.stringify({
      "event.name": "flaggo.service.started",
      "server.address": "http://0.0.0.0:5011",
    })),
    undefined,
  );
});

test("structured log observer captures complete JSON objects", () => {
  const entries = [];
  const observer = createStructuredLogObserver(
    () => {},
    (entry) => entries.push(entry),
  );

  observer.write("not json\n{\"event.name\":\"flaggo.service.");
  observer.write(
    "started\",\"flaggo.materializer.active_route_count\":2}\n",
  );
  observer.end();

  assert.deepEqual(entries, [{
    "event.name": "flaggo.service.started",
    "flaggo.materializer.active_route_count": 2,
  }]);
});

test("materialization telemetry projects worker health", () => {
  assert.deepEqual(
    parseMaterializationHealth({
      "flaggo.materializer.checkpoint_batch_id": "6",
      "flaggo.materializer.pending_batch_count": 0,
      "flaggo.materializer.observed_at": "2026-10-05T15:00:00Z",
      "flaggo.materializer.oldest_pending_received_at": "",
      "flaggo.materializer.newest_pending_received_at": "",
      "flaggo.materializer.oldest_pending_age_ms": 0,
      "flaggo.materializer.newest_evidence_observed_at_unix_nano": "1000",
      "flaggo.materializer.evidence_freshness_ms": 10,
    }),
    {
      checkpointBatchId: 6,
      evidenceFreshnessMilliseconds: 10,
      newestEvidenceObservedAtUnixNano: "1000",
      newestPendingReceivedAt: null,
      observedAt: "2026-10-05T15:00:00Z",
      oldestPendingAgeMilliseconds: null,
      oldestPendingReceivedAt: null,
      pendingBatchCount: 0,
    },
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
