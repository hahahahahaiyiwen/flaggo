import { createHash } from "node:crypto";
import { canonicalize } from "json-canonicalize";
import { record } from "./static-schema.js";

import type {
  DecisionDefinition,
  DecisionDefinitionBundle,
  Sha256Digest,
} from "./types.js";

export function compareCanonicalStrings(left: string, right: string): number {
  const leftPoints = [...left].map((value) => value.codePointAt(0)!);
  const rightPoints = [...right].map((value) => value.codePointAt(0)!);
  const length = Math.min(leftPoints.length, rightPoints.length);
  for (let index = 0; index < length; index += 1) {
    const difference = leftPoints[index]! - rightPoints[index]!;
    if (difference !== 0) return difference;
  }
  return leftPoints.length - rightPoints.length;
}

export function normalizeDefinition(
  definition: DecisionDefinition,
  semanticIdentity = true,
): DecisionDefinition {
  const normalized = structuredClone(definition);
  if (semanticIdentity) delete normalized.owner;
  for (const key of ["context", "inputs", "evidence"] as const) {
    if (Object.hasOwn(normalized, key) && record(normalized[key]) === undefined) {
      throw new Error(`${key} must be a map`);
    }
  }
  normalized.context = Object.fromEntries(
    Object.entries(normalized.context ?? {}).map(([key, field]) => [
      key, { ...field, required: field.required ?? false },
    ]),
  );
  normalized.inputs ??= {};
  normalized.evidence ??= {};
  if (normalized.policy.kind === "inline") {
    if (normalized.policy.clientFallback?.requiredEvidenceUnavailable === "forbid") {
      delete normalized.policy.clientFallback;
    }
    const kinds = new Set<string>();
    for (const constraint of normalized.policy.constraints) {
      if (kinds.has(constraint.kind)) {
        throw new Error(`duplicate inline policy constraint kind: ${constraint.kind}`);
      }
      kinds.add(constraint.kind);
    }
    normalized.policy.constraints.sort((left, right) =>
      compareCanonicalStrings(left.kind, right.kind)
    );
  }
  return normalized;
}

function digest(value: unknown): Sha256Digest {
  return `sha256:${createHash("sha256").update(canonicalize(value)).digest("hex")}`;
}

export function contractDigest(
  key: string,
  definition: DecisionDefinition,
): Sha256Digest {
  return digest({ key, contract: normalizeDefinition(definition) });
}

export function normalizeBundle(
  bundle: DecisionDefinitionBundle,
): DecisionDefinitionBundle {
  if (bundle.format !== "flaggo.decision-definition-bundle/v2" || record(bundle.decisions) === undefined) {
    throw new Error("a v2 manifest with a decisions map is required");
  }
  return {
    ...structuredClone(bundle),
    decisions: Object.fromEntries(
      Object.entries(bundle.decisions).map(([key, definition]) => [
        key, normalizeDefinition(definition, false),
      ]),
    ),
  };
}

export function bundleDigest(bundle: DecisionDefinitionBundle): Sha256Digest {
  return digest(normalizeBundle(bundle));
}
