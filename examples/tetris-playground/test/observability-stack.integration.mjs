import assert from "node:assert/strict";
import { once } from "node:events";
import { access, mkdtemp, rm, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";

import { createHostLifecycle, waitForReady } from "../../tetris-integration/host-process.mjs";
import { startObservabilityStack } from "../local-stack.mjs";

test("collector reaches loopback ingestion and leaves removable retained LGTM data",
  { timeout: 240_000 }, async (context) => {
    const runDirectory = await mkdtemp(resolve(tmpdir(), "flaggo-observability-"));
    const lifecycle = createHostLifecycle({ removeRunDirectory: async () => {} });
    const received = [];
    const ingestion = createServer((request, response) => {
      const chunks = [];
      request.on("data", (chunk) => chunks.push(chunk));
      request.on("end", () => {
        const body = Buffer.concat(chunks);
        received.push({
          path: request.url,
          body: request.headers["content-encoding"] === "gzip" ? gunzipSync(body) : body,
        });
        response.writeHead(200, { "content-type": "application/json" }).end("{}");
      });
    });
    context.after(async () => {
      try {
        await lifecycle.cleanup();
      } finally {
        ingestion.closeAllConnections();
        await new Promise((resolvePromise, reject) =>
          ingestion.close((error) => error ? reject(error) : resolvePromise()));
        await rm(runDirectory, { recursive: true, force: true });
      }
    });
    ingestion.listen(0, "127.0.0.1");
    await once(ingestion, "listening");
    const { endpoints, host } = await startObservabilityStack({
      exampleDirectory: resolve(dirname(fileURLToPath(import.meta.url)), ".."),
      ingestionPort: ingestion.address().port,
      lifecycle,
      retain: true,
      runDirectory,
    });
    for (const [url, marker] of [
      [endpoints.applicationOtlpUrl, "application-route-probe"],
      [endpoints.internalOtlpUrl, "internal-route-probe"],
    ]) {
      const response = await fetch(`${url}/v1/logs`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          resourceLogs: [{
            resource: { attributes: [{
              key: "service.name", value: { stringValue: "flaggo-route-probe" },
            }] },
            scopeLogs: [{ logRecords: [{
              timeUnixNano: String(BigInt(Date.now()) * 1_000_000n),
              severityNumber: 9,
              body: { stringValue: marker },
            }] }],
          }],
        }),
        signal: AbortSignal.timeout(5_000),
      });
      assert.equal(response.status, 200);
      await response.text();
    }
    await waitForReady(async (signal) => {
      assert.ok(received.some(({ path, body }) =>
        path === "/v1/logs" && body.includes("application-route-probe")));
      const query = encodeURIComponent('{service_name="flaggo-route-probe"}');
      const response = await fetch(`${endpoints.lokiUrl}/loki/api/v1/query_range?query=${query}`, { signal });
      assert.equal(response.status, 200);
      const result = await response.text();
      assert.ok(result.includes("application-route-probe"));
      assert.ok(result.includes("internal-route-probe"));
      return true;
    }, host, lifecycle.signal, { timeoutMilliseconds: 30_000, retryDelayMilliseconds: 250 });
    assert.ok(!received.some(({ body }) => body.includes("internal-route-probe")));

    await lifecycle.cleanup();
    if (process.platform === "linux") {
      const data = await stat(resolve(runDirectory, "lgtm-data", "tempo"));
      assert.equal(data.uid, process.getuid());
      assert.equal(data.gid, process.getgid());
    }
    await rm(runDirectory, { recursive: true });
    await assert.rejects(access(runDirectory), { code: "ENOENT" });
  });
