import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  InvalidFlaggoInputError,
} from "@flaggo/sdk/configuration";
import { deployContracts } from "../../contract-deployment.mjs";

const contractDigest =
  "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const executableDigest =
  "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const authority = {
  tenant: "local",
  application: "deployment-test",
  environment: "test",
};
const services = {
  contractServiceUrl: "https://contracts.test",
  decisionServiceUrl: "https://decisions.test",
  otlpIngestionUrl: "https://telemetry.test",
};
const contract = {
  name: "demo.interval",
  expression_syntax: "flaggo.cel/v1",
  attributes: [],
  result: {
    schema: {
      type: "number",
      minimum: 200,
      maximum: 1_500,
      multipleOf: 50,
    },
    default: 800,
  },
};
const deployedContract = {
  ...contract,
  authority,
};

async function fixture(fileName = "demo.interval.decision-contract.json") {
  const directory = await mkdtemp(join(tmpdir(), "flaggo-deploy-"));
  const contractsDirectory = join(directory, "flaggo", "contracts");
  await mkdir(contractsDirectory, { recursive: true });
  await writeFile(
    join(directory, "flaggo.deploy.json"),
    `${JSON.stringify({
      format: "flaggo.deploy/v2",
      authority,
      contracts: [`flaggo/contracts/${fileName}`],
    }, null, 2)}\n`,
  );
  await writeFile(
    join(contractsDirectory, fileName),
    `${JSON.stringify(contract, null, 2)}\n`,
  );
  return {
    directory,
    manifestPath: join(directory, "flaggo.deploy.json"),
  };
}

test("deployment manifest deploys each contract with one PUT", async () => {
  const files = await fixture();
  const requests = [];
  try {
    const result = await deployContracts({
      manifestPath: files.manifestPath,
      services,
      fetch: async (input, init) => {
        requests.push({ input, init });
        return new Response(JSON.stringify({
          name: contract.name,
          contractDigest,
          status: "ready",
          acceptedAt: "2026-09-27T12:00:00Z",
          activeExecutableDigest: executableDigest,
          contract: deployedContract,
        }), {
          status: 201,
          headers: { "Content-Type": "application/json" },
        });
      },
    });

    assert.equal(requests.length, 1);
    assert.equal(requests[0].init.method, "PUT");
    assert.equal(
      String(requests[0].input),
      "https://contracts.test/v3/decision-contracts/demo.interval",
    );
    assert.deepEqual(
      JSON.parse(String(requests[0].init.body)),
      deployedContract,
    );
    assert.deepEqual(result.contracts[0].contract, deployedContract);
    assert.equal(result.contracts[0].deployment.contractDigest, contractDigest);
    assert.deepEqual(JSON.parse(JSON.stringify(result.runtimeConfig)), {
      format: "flaggo.runtime-config/v1",
      authority,
      services,
      bindings: {
        "demo.interval": { contractDigest },
      },
    });
    assert.equal(Object.isFrozen(result.runtimeConfig), true);
    assert.equal(Object.getPrototypeOf(result.runtimeConfig.bindings), null);
  } finally {
    await rm(files.directory, { recursive: true, force: true });
  }
});

test("deployment requires the filename to match contract.name", async () => {
  const files = await fixture("wrong-name.decision-contract.json");
  let requestCount = 0;
  try {
    await assert.rejects(
      () => deployContracts({
        manifestPath: files.manifestPath,
        services,
        fetch: async () => {
          requestCount += 1;
          throw new Error("Unexpected request.");
        },
      }),
      /must be named 'demo\.interval\.decision-contract\.json'/u,
    );
    assert.equal(requestCount, 0);
  } finally {
    await rm(files.directory, { recursive: true, force: true });
  }
});

test("deployment rejects unusable service URLs before the first PUT", async () => {
  const files = await fixture();
  let requestCount = 0;
  const invalidUrls = [
    "https://decisions.test?",
    "https://decisions.test#",
    "https://decisions.test:0",
    "https://decisions.test:65536",
    `https://decisions.test/${"x".repeat(2_048)}`,
  ];
  try {
    for (const decisionServiceUrl of invalidUrls) {
      await assert.rejects(
        () => deployContracts({
          manifestPath: files.manifestPath,
          services: {
            ...services,
            decisionServiceUrl,
          },
          fetch: async () => {
            requestCount += 1;
            throw new Error("Unexpected request.");
          },
        }),
        InvalidFlaggoInputError,
      );
    }
    assert.equal(requestCount, 0);
  } finally {
    await rm(files.directory, { recursive: true, force: true });
  }
});
