import { compileManifest } from "../src/manifest.js";
import type {
  DecisionDefinitionBundle, EvidenceBinding, RegistrationReceipt, ServerDecisionPayload,
} from "../src/types.js";

export const gauge: EvidenceBinding = {
  meaning: "Latest reported occupancy, not a population average.",
  type: "number", unit: "1", range: [0, 1],
  source: {
    kind: "metric", name: "queue.occupancy", dataType: "gauge",
    scope: { name: "worker" }, resourceAttributes: { "service.name": "worker" },
    value: { from: "value" },
  },
  projection: { kind: "latest" }, target: { type: "global" },
  freshness: { maxAgeSeconds: 30 }, sampling: { accept: "observed" },
  attribution: { kind: "none" },
};

export function manifest(): DecisionDefinitionBundle {
  return {
    format: "flaggo.decision-definition-bundle/v2",
    application: { id: "worker", environment: "test" },
    decisions: {
      parallelism: {
        result: { type: "number", min: 1, max: 10, default: 2 },
        targeting: { hierarchy: ["global"], primary: "global", fallbackOrder: [] },
        inputs: { occupancy: { source: "evidence", binding: "occupancy" } },
        evidence: { occupancy: structuredClone(gauge) },
        policy: { kind: "inline", constraints: [] },
      },
    },
  };
}

export function compiled(input = manifest()) {
  const artifacts = compileManifest(input);
  const receipt: RegistrationReceipt = {
    application: input.application.id, environment: input.application.environment,
    bundleDigest: artifacts.catalog.bundleDigest,
    acceptedDefinitions: Object.fromEntries(Object.entries(artifacts.catalog.decisions).map(([key, value]) => [
      key, { definitionId: `def-${key}`, revision: "revision-1", contractDigest: value.contractDigest },
    ])),
    status: "approved", compatibility: "new-contract-required", issues: [],
  };
  return { ...artifacts, receipt };
}

export function serverDecision(receipt: RegistrationReceipt): ServerDecisionPayload<number> {
  const identity = receipt.acceptedDefinitions.parallelism!;
  return {
    decisionKey: "parallelism",
    definition: {
      appId: receipt.application, environment: receipt.environment, key: "parallelism",
      definitionId: identity.definitionId, revision: identity.revision,
    },
    decisionId: "decision-1", value: 4, valueType: "number",
    decisionMode: "strategy", strategyId: "rule-1", confidence: null,
    controlTarget: { type: "global", id: "global" },
    targetProvenance: [], resolutionChain: ["global"],
    definitionStatus: { ...identity, bundleDigest: receipt.bundleDigest, integrity: "verified" },
    exposure: { confirmationRequired: true, confirmToken: "confirmation-1" },
    reason: "numeric rule", auditId: "audit-1",
    fallback: { source: "server", resolutionFallbackUsed: false, decisionFallbackUsed: false, reason: null },
    policy: { result: "approved", reasons: [], appliedConstraints: [] },
  };
}

export function response(status: number, body: unknown, contentType = "application/json"): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": contentType } });
}
