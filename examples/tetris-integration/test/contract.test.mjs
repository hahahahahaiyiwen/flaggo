import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const exampleDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
);

test("integration and playground package the same adaptive decision contract", async () => {
  const [integration, playground] = await Promise.all([
    readFile(resolve(exampleDirectory,
      "flaggo/contracts/tetris.dropInterval.decision-contract.json"), "utf8"),
    readFile(resolve(exampleDirectory,
      "../tetris-playground/flaggo/contracts/tetris.dropInterval.decision-contract.json"), "utf8"),
  ]);
  assert.deepEqual(JSON.parse(integration), JSON.parse(playground));
});

test("adaptive contract keeps current gravity without data and explores calibrated arms", async () => {
  const contract = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "flaggo/contracts/tetris.dropInterval.decision-contract.json",
  ), "utf8"));

  assert.deepEqual(contract.authoredExecutable.rules, [
    {
      name: "retain-current-without-observations",
      when: { expression: "attributes.active_play_ms_5s == 0.0 || attributes.pieces_locked_5s == 0" },
      return: { expression: "attributes.current_drop_interval_ms" },
    },
    {
      name: "explore-fast-gravity",
      when: { expression: "attributes._random < 0.5" },
      return: { value: 200 },
    },
    {
      name: "explore-slow-gravity",
      when: { expression: "true" },
      return: { value: 1_000 },
    },
  ]);
  assert.equal(contract.attributes.length, 12);
  assert.ok(!contract.attributes.some((attribute) =>
    ["current_level", "recovery_failures_5s", "_random"].includes(attribute.name)));
  assert.deepEqual(contract.learning.objective.primary,
    { evidence: "challenge_balance", direction: "maximize" });
  assert.equal(contract.learning.evidence.length, 4);
  for (const evidence of contract.learning.evidence) {
    assert.deepEqual(evidence.correlateBy, ["session_id", "exposure_id"]);
    assert.equal(evidence.source.scope, "tetris.experience");
    assert.equal(evidence.source.metricKind, "gauge");
    assert.equal(evidence.source.correlation.exposure_id.attribute, "tetris.exposure.id");
    assert.ok(contract.attributes.some((attribute) => attribute.name === evidence.attribute));
  }
});

test("live learning preserves the online contract but has mixed-source ongoing-window guidance", async () => {
  const load = async (name) => JSON.parse(await readFile(resolve(
    exampleDirectory, "../tetris-playground/flaggo/contracts", name), "utf8"));
  const manifest = JSON.parse(await readFile(resolve(
    exampleDirectory, "../tetris-playground/flaggo.live.deploy.json"), "utf8"));
  assert.equal(manifest.contracts.length, 1);
  const [benchmark, live] = await Promise.all([
    load("tetris.dropInterval.decision-contract.json"),
    readFile(resolve(exampleDirectory, "../tetris-playground", manifest.contracts[0]), "utf8")
      .then(JSON.parse),
  ]);
  assert.equal(basename(manifest.contracts[0]), `${live.name}.decision-contract.json`);
  for (const field of ["name", "expression_syntax", "attributes", "result"]) {
    assert.deepEqual(live[field], benchmark[field]);
  }
  assert.deepEqual(live.authoredExecutable.rules[0], benchmark.authoredExecutable.rules[0]);
  assert.deepEqual(live.learning.objective, benchmark.learning.objective);
  assert.deepEqual(live.learning.policy, benchmark.learning.policy);
  assert.deepEqual(live.learning.evidence.map(({ description, ...binding }) => binding),
    benchmark.learning.evidence.map(({ description, ...binding }) => binding));
  const guidance = live.learning.evidence[0].description;
  assert.match(guidance, /dataset=live/u);
  assert.match(guidance, /origin=human or greedy/u);
  assert.match(guidance, /no nine-completed-episode gate/u);
  assert.match(guidance, /three distinct applied exposures/u);
});

test("live authored exploration has five equal-width arms and preserves no-history gravity", async () => {
  const live = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "../tetris-playground/flaggo/contracts/live/tetris.dropInterval.decision-contract.json",
  ), "utf8"));
  assert.deepEqual(live.authoredExecutable.rules.slice(1), [
    { name: "explore-fast-gravity", when: { expression: "attributes._random < 0.2" }, return: { value: 200 } },
    { name: "explore-400ms-gravity", when: { expression: "attributes._random < 0.4" }, return: { value: 400 } },
    { name: "explore-600ms-gravity", when: { expression: "attributes._random < 0.6" }, return: { value: 600 } },
    { name: "explore-800ms-gravity", when: { expression: "attributes._random < 0.8" }, return: { value: 800 } },
    { name: "explore-slow-gravity", when: { expression: "true" }, return: { value: 1_000 } },
  ]);
  // These authored CEL expressions use the same scalar operators as JavaScript.
  const select = (attributes) => {
    const rule = live.authoredExecutable.rules.find((candidate) =>
      new Function("attributes", `return (${candidate.when.expression});`)(attributes));
    assert.ok(rule);
    return rule.return.value
      ?? new Function("attributes", `return (${rule.return.expression});`)(attributes);
  };
  const history = { active_play_ms_5s: 5_000, pieces_locked_5s: 2, current_drop_interval_ms: 650 };
  for (const [random, interval] of [
    [0, 200], [0.2 - Number.EPSILON, 200], [0.2, 400],
    [0.4 - Number.EPSILON, 400], [0.4, 600],
    [0.6 - Number.EPSILON, 600], [0.6, 800],
    [0.8 - Number.EPSILON, 800], [0.8, 1_000], [1 - Number.EPSILON, 1_000],
  ]) {
    assert.equal(select({ ...history, _random: random }), interval);
    assert.equal(select({ ...history, _random: random, active_play_ms_5s: 0 }), 650);
    assert.equal(select({ ...history, _random: random, pieces_locked_5s: 0 }), 650);
  }
  assert.match(live.learning.evidence[0].description,
    /History-bearing 800ms exploration is eligible/u);
});

test("live guidance authorizes bot-proxy proposals without a human comparison gate", async () => {
  const live = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "../tetris-playground/flaggo/contracts/live/tetris.dropInterval.decision-contract.json",
  ), "utf8"));
  const guidance = live.learning.evidence[0].description;
  assert.match(guidance, /Greedy-bot evidence is an intentional proxy for choosing the human-facing policy/u);
  assert.match(guidance, /Human comparison data is not required/u);
  assert.match(guidance, /not measured human preference or verified human enjoyment/u);
  assert.match(guidance, /a modest positive observed gain/u);
  assert.match(guidance, /lack of statistical significance do not independently block a Candidate/u);
  assert.match(guidance, /No Change only when eligible comparisons support retaining the active policy/u);
});

test("live proposal guidance preserves eligibility and matches comparisons to rule scope", async () => {
  const live = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "../tetris-playground/flaggo/contracts/live/tetris.dropInterval.decision-contract.json",
  ), "utf8"));
  const guidance = live.learning.evidence[0].description;
  assert.match(guidance, /Establish actual successful SDK selections by session AND exposure IDs/u);
  assert.match(guidance, /Exclude no-history selections/u);
  assert.match(guidance, /positive scored duration/u);
  assert.match(guidance, /zero-duration terminal points/u);
  assert.match(guidance, /at least three distinct applied exposures per observed interval/u);
  assert.match(guidance, /A global choice may compare all eligible windows/u);
  assert.match(guidance, /contextual rules require matching gameplay-context cohorts/u);
  assert.match(guidance, /Preserve the no-observations retain-current rule/u);
  assert.match(guidance, /objective guardrails for the affected population/u);
});

test("live guidance permits inferred unobserved intervals without claiming measured safety", async () => {
  const live = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "../tetris-playground/flaggo/contracts/live/tetris.dropInterval.decision-contract.json",
  ), "utf8"));
  const guidance = live.learning.evidence[0].description;
  assert.match(guidance, /any result-schema-valid interval: 200\.\.1500ms in 50ms steps/u);
  assert.match(guidance, /including previously unobserved values such as 500 or 700ms/u);
  assert.match(guidance, /not a Candidate whitelist/u);
  assert.match(guidance, /minimum applies to observed supporting data, not to a previously unobserved proposed interval/u);
  assert.match(guidance, /interpolation, extrapolation, or gameplay-mechanism reasoning/u);
  assert.match(guidance, /distinguish measured results from predictions/u);
  assert.match(guidance, /expected objective effect and each guardrail/u);
  assert.match(guidance, /not empirically verified before auto-activation/u);
  assert.match(guidance, /Unobserved status alone is not a reason for No Candidate/u);
  assert.match(guidance, /measured or predicted guardrail violations for the proposed behavior mean No Candidate/u);
  assert.doesNotMatch(guidance, /narrow choice from observed arms|Do not invent outcomes, forced winners, new exploration/u);
  const benchmark = JSON.parse(await readFile(resolve(
    exampleDirectory,
    "../tetris-playground/flaggo/contracts/tetris.dropInterval.decision-contract.json",
  ), "utf8"));
  assert.match(benchmark.learning.evidence[0].description,
    /Compare empirical choices from the authored 200\/1000ms arms/u);
  assert.doesNotMatch(benchmark.learning.evidence[0].description,
    /Candidates may choose any result-schema-valid interval/u);
});
