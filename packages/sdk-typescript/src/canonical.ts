import { createHash } from "node:crypto";
import { canonicalize } from "json-canonicalize";

import type {
  DecisionDefinition,
  DecisionDefinitionBundle,
  Sha256Digest,
  SignalDeclaration,
  SignalRef,
} from "./types.js";

function clone<T>(value: T): T {
  return structuredClone(value);
}

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

function signalRefs(values: SignalRef[]): SignalRef[] {
  const keys = new Set<string>();
  for (const value of values) {
    if (keys.has(value.key)) {
      throw new Error(`duplicate signal reference: ${value.key}`);
    }
    keys.add(value.key);
  }
  return [...keys].sort(compareCanonicalStrings).map((key) => ({ key }));
}

function objectiveSignalKeys(value: unknown, keys: Set<string>): void {
  if (Array.isArray(value)) {
    for (const item of value) objectiveSignalKeys(item, keys);
    return;
  }
  if (value === null || typeof value !== "object") return;
  const record = value as Record<string, unknown>;
  const signal = record.signal;
  if (
    signal !== null
    && typeof signal === "object"
    && typeof (signal as Record<string, unknown>).key === "string"
  ) {
    keys.add((signal as { key: string }).key);
  }
  for (const item of Object.values(record)) objectiveSignalKeys(item, keys);
}

export function normalizeDefinition(
  definition: DecisionDefinition,
  semanticIdentity = true,
): DecisionDefinition {
  const normalized = clone(definition) as DecisionDefinition & Record<string, unknown>;
  delete normalized.contractDigest;
  delete normalized.revision;
  delete normalized.schemaDigest;
  if (semanticIdentity) {
    delete normalized.definitionId;
    delete normalized.owner;
  }

  const keys = new Set<string>();
  for (const role of ["evidence", "guardrails"] as const) {
    const values = normalized.signals?.[role];
    if (values !== undefined) {
      normalized.signals![role] = signalRefs(values);
      for (const value of values) keys.add(value.key);
    }
  }
  if (normalized.inference?.inputs !== undefined) {
    normalized.inference.inputs = signalRefs(normalized.inference.inputs);
    for (const value of normalized.inference.inputs) keys.add(value.key);
  }
  objectiveSignalKeys(normalized.intent, keys);

  if (normalized.signals !== undefined || keys.size > 0) {
    normalized.signals ??= {};
    if (keys.size > 0) {
      normalized.signals.allowed = [...keys]
        .sort(compareCanonicalStrings)
        .map((key) => ({ key }));
    } else {
      delete normalized.signals.allowed;
    }
    if (Object.keys(normalized.signals).length === 0) delete normalized.signals;
  }

  if (normalized.policy.kind === "inline") {
    if (
      normalized.policy.clientFallback?.requiredEvidenceUnavailable === "forbid"
    ) {
      delete normalized.policy.clientFallback;
    }
    const constraints = normalized.policy.constraints ?? [];
    const kinds = new Set<string>();
    for (const constraint of constraints) {
      if (kinds.has(constraint.kind)) {
        throw new Error(`duplicate inline policy constraint kind: ${constraint.kind}`);
      }
      kinds.add(constraint.kind);
    }
    normalized.policy.constraints = [...constraints].sort((left, right) => {
      const kindOrder = compareCanonicalStrings(left.kind, right.kind);
      return kindOrder !== 0
        ? kindOrder
        : compareCanonicalStrings(canonicalize(left), canonicalize(right));
    });
  }
  return normalized;
}

function digest(value: unknown): Sha256Digest {
  const bytes = canonicalize(value);
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

export function contractDigest(definition: DecisionDefinition): Sha256Digest {
  return digest(normalizeDefinition(definition));
}

export function signalSchemaDigest(declaration: SignalDeclaration): Sha256Digest {
  const normalized = clone(declaration);
  delete normalized.schemaDigest;
  return digest(normalized);
}

export function normalizeBundle(
  bundle: DecisionDefinitionBundle,
): DecisionDefinitionBundle {
  const normalized = clone(bundle);
  const signals = new Map<string, SignalDeclaration>();
  for (const declaration of normalized.signals ?? []) {
    const computed = signalSchemaDigest(declaration);
    if (
      declaration.schemaDigest !== undefined
      && declaration.schemaDigest !== computed
    ) {
      throw new Error(`schema digest mismatch for signal key: ${declaration.key}`);
    }
    const candidate = clone(declaration);
    delete candidate.schemaDigest;
    const previous = signals.get(candidate.key);
    if (previous !== undefined) {
      if (signalSchemaDigest(previous) !== computed) {
        throw new Error(`conflicting signal declaration for key: ${candidate.key}`);
      }
      throw new Error(`duplicate signal declaration: ${candidate.key}`);
    }
    signals.set(candidate.key, candidate);
  }
  if (normalized.signals !== undefined) {
    normalized.signals = [...signals]
      .sort(([left], [right]) => compareCanonicalStrings(left, right))
      .map(([, declaration]) => declaration);
  }

  const definitions = new Map<string, DecisionDefinition>();
  for (const definition of normalized.definitions) {
    const candidate = normalizeDefinition(definition, false);
    const previous = definitions.get(candidate.key);
    if (previous !== undefined) {
      if (contractDigest(previous) !== contractDigest(candidate)) {
        throw new Error(`conflicting decision definition key: ${candidate.key}`);
      }
      if (canonicalize(previous) !== canonicalize(candidate)) {
        throw new Error(
          `metadata-conflicting duplicate decision key: ${candidate.key}`,
        );
      }
      continue;
    }
    definitions.set(candidate.key, candidate);
  }
  normalized.definitions = [...definitions]
    .sort(([left], [right]) => compareCanonicalStrings(left, right))
    .map(([, definition]) => definition);
  return normalized;
}

export function bundleDigest(bundle: DecisionDefinitionBundle): Sha256Digest {
  return digest(normalizeBundle(bundle));
}
