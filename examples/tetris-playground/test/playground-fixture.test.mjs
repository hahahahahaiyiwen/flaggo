import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { readAttemptFailureRecords, restartHumanSession } from "./playground-fixture.mjs";

test("fresh-session setup surfaces failed or ineffective restarts", async () => {
  const before = { human: { state: { sessionId: "old" } } };
  for (const outcome of ["http-failure", "unchanged-session"]) {
    await assert.rejects(restartHumanSession({
      async get() { return { ok: () => true, json: async () => before }; },
      async post() {
        return {
          ok: () => outcome !== "http-failure", status: () => 503,
          json: async () => ({
            human: { state: { sessionId: "old", status: "playing", gameOver: false, paused: false, score: 0 } },
          }),
        };
      },
    }, "http://playground.test"), outcome === "http-failure"
      ? /HTTP 503/u : /fresh playing session/u);
  }
});

async function fixture(context) {
  const root = await mkdtemp(resolve(tmpdir(), "flaggo-failure-records-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

test("captures exact attempt failures without reading unrelated or provider session files", async (context) => {
  const root = await fixture(context);
  const record = {
    schemaVersion: 1, attemptId: "attempt-a",
    recordedAt: "2026-01-01T00:00:00Z",
    error: "invalid analysis state: draft does not match handoff digest",
  };
  const path = resolve(root, "analysis-workspaces", "contract", "cycles", "cycle", "attempts", "attempt-a-failure.json");
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, JSON.stringify(record), "utf8");
  const unrelated = resolve(root, "analysis-workspaces", "contract", "cycles", "cycle", "analysis", ".copilot", "session-failure.json");
  await mkdir(dirname(unrelated), { recursive: true });
  await writeFile(unrelated, "not a failure record", "utf8");
  const result = await readAttemptFailureRecords(root);
  assert.equal(result.workspaceAvailable, true);
  assert.equal(result.failures.length, 1);
  assert.deepEqual(result.failures[0].record, record);
  assert.ok(result.failures[0].path.endsWith("attempt-a-failure.json"));
});

test("explicitly reports when an analysis workspace was not created", async (context) => {
  assert.deepEqual(await readAttemptFailureRecords(await fixture(context)), {
    workspaceAvailable: false, failures: [],
  });
});

test("does not silently discard a malformed failure record", async (context) => {
  const root = await fixture(context);
  const path = resolve(root, "analysis-workspaces", "contract", "cycles", "cycle", "attempts", "attempt-a-failure.json");
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, "invalid JSON", "utf8");
  await assert.rejects(readAttemptFailureRecords(root), SyntaxError);
});
