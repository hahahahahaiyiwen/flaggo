import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { createDecisionClient } from "@flaggo/sdk/runtime";
import { loadDeployedTarget } from "./deployed-target.mjs";

export async function verifyDecisions(target, authority, expectedDefault) {
  const { contractServiceUrl, decisionServiceUrl } = target.services;
  const contractDigest = target.identity.contractDigest;
  const base = `/v3/decision-contracts/tetris.dropInterval/versions/${contractDigest}`;
  const catalogResponse = await fetch(`${contractServiceUrl}${base}/executables`);
  assert.equal(catalogResponse.status, 200);
  const catalog = await catalogResponse.json();
  const runtimeConfig = {
    format: "flaggo.runtime-config/v1", authority, services: target.services,
    bindings: { "tetris.dropInterval": { contractDigest } },
  };
  const client = createDecisionClient({ runtimeConfig, random: () => 0.25 });
  const decisions = [];
  for (const pressure of [0.1, 0.3]) {
    const attributes = {
      board_pressure_mean_5s: pressure, board_pressure_max_5s: pressure,
      hard_drop_ratio_5s: 0.5, placement_time_mean_ms_5s: 3000,
      pieces_locked_5s: 2, active_play_ms_5s: 5000,
      challenge_balance_5s: 0.5, lines_cleared_5s: 0, game_over_count_5s: 0,
      current_drop_interval_ms: 800, session_id: "deployment-parity",
      exposure_id: `unused-${pressure}`,
    };
    const sdk = (await client.decide("tetris.dropInterval", { attributes })).value;
    const response = await fetch(`${decisionServiceUrl}${base}/decisions`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ attributes: { ...attributes, _random: 0.25 } }),
    });
    assert.equal(response.status, 200);
    const rest = await response.json();
    for (const decision of [sdk, rest]) {
      assert.equal(decision.contractDigest, contractDigest);
      assert.equal(decision.executableDigest, catalog.activeExecutable.executableDigest);
      if (expectedDefault !== undefined) assert.deepEqual(decision.result, expectedDefault);
    }
    assert.deepEqual(sdk.result, rest.result);
    assert.deepEqual(sdk.evaluation, rest.evaluation);
    decisions.push({ attributes, sdk, rest });
  }
  return { runtimeConfig, activeExecutable: catalog.activeExecutable, decisions };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const target = await loadDeployedTarget(
    process.argv[2], process.env.FLAGGO_DEPLOYED_SMOKE_CONFIRM,
  );
  const manifest = JSON.parse(await readFile(
    new URL("../../examples/tetris-playground/flaggo.deploy.json", import.meta.url), "utf8",
  ));
  await writeFile(process.argv[3], JSON.stringify(
    await verifyDecisions(target, manifest.authority), null, 2,
  ));
}
