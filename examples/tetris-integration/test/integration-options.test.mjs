import assert from "node:assert/strict";
import test from "node:test";

import { parseIntegrationArguments } from "../integration-options.mjs";
import { startLocalFlaggoHosts } from "../local-flaggo.mjs";
import { createCandidateObserver } from "../analysis-acceptance.mjs";

test("integration defaults to Rust hosts without analysis; credentialed analysis stays opt-in", () => {
  assert.deepEqual(parseIntegrationArguments([]), {
    analysisMode: undefined,
  });
  assert.deepEqual(parseIntegrationArguments(["--analysis"]), {
    analysisMode: "copilot",
  });
});

test("full-loop acceptance explicitly selects credential-free analysis", () => {
  assert.deepEqual(parseIntegrationArguments(["--deterministic-analysis"]), {
    analysisMode: "deterministic",
  });
});

test("conflicting analysis providers and unknown options fail before startup", () => {
  for (const arguments_ of [
    ["--analysis", "--deterministic-analysis"],
    ["--deterministic-analysis", "--analysis"],
    ["--rust"],
    ["--rust-core"],
    ["--dotnet-core"],
  ]) {
    assert.throws(() => parseIntegrationArguments(arguments_));
  }
});

test("invalid provider configuration fails before creating a run", async () => {
  await assert.rejects(
    startLocalFlaggoHosts({ analysisMode: "unknown" }),
    /analysisMode/u,
  );
});

const correlation = {
  "flaggo.analysis.cycle.id": "cycle",
  "flaggo.analysis.attempt.id": "attempt",
};
const draftHandoff = {
  ...correlation,
  "event.name": "flaggo.analysis.action.completed",
  "flaggo.analysis.action.kind": "session.run",
  "flaggo.analysis.action.outcome": "handoff",
};
const recoverable = {
  ...correlation,
  "event.name": "flaggo.analysis.cycle.failed",
  "flaggo.operation.outcome": "recoverable",
  "flaggo.failure.category": "internal",
};

test("deterministic analysis waits through a correlated draft checkpoint, not a terminal success", () => {
  const observer = createCandidateObserver({ allowDraftHandoff: true });
  assert.equal(observer.predicate(draftHandoff), false);
  assert.equal(observer.predicate(recoverable), false);
  assert.deepEqual(observer.handoffs, [recoverable]);
  assert.equal(observer.predicate({
    "event.name": "flaggo.analysis.cycle.completed",
    "flaggo.operation.outcome": "candidate",
  }), true);
  assert.throws(() => observer.predicate(recoverable), /Async Analysis failed/u);
});

test("handoffs never mask unrelated attempts, genuine failures or missing correlation", () => {
  for (const failure of [
    { ...recoverable, "flaggo.analysis.attempt.id": "other" },
    { ...recoverable, "flaggo.analysis.cycle.id": "other" },
    { ...recoverable, "flaggo.failure.category": "dependency" },
    { ...recoverable, "event.name": "flaggo.service.failed" },
    { ...recoverable, "flaggo.analysis.attempt.id": undefined },
  ]) {
    const observer = createCandidateObserver({ allowDraftHandoff: true });
    observer.predicate(draftHandoff);
    assert.throws(() => observer.predicate(failure), /Async Analysis failed/u);
  }
  assert.throws(
    () => createCandidateObserver({ allowDraftHandoff: true }).predicate(recoverable),
    /Async Analysis failed/u,
  );
  const strict = createCandidateObserver();
  strict.predicate(draftHandoff);
  assert.throws(() => strict.predicate(recoverable), /Async Analysis failed/u);
});

test("no-change, no-candidate, supersession and other terminal outcomes cannot pass", () => {
  for (const outcome of ["no_change", "no_candidate", "superseded", "idle", "failure"]) {
    assert.throws(() => createCandidateObserver({ allowDraftHandoff: true }).predicate({
      "event.name": "flaggo.analysis.cycle.completed",
      "flaggo.operation.outcome": outcome,
    }), /did not produce a Candidate/u);
  }
});
