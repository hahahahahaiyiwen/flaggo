import { randomUUID } from "node:crypto";

import {
  bundleDigest,
  compareCanonicalStrings,
  contractDigest,
  normalizeBundle,
} from "./canonical.js";
import {
  ContractConflictError,
  FlaggoHttpError,
  InvalidServerResponseError,
  MissingAcceptedDefinitionError,
  RequiresApprovalError,
} from "./errors.js";
import type {
  AcceptedDefinition,
  ClientFallbackResult,
  DecisionDefinitionBundle,
  DecisionReceipt,
  DecisionResult,
  ExposureConfirmationResult,
  FetchLike,
  NumberTuneRequest,
  ProblemDetails,
  RegistrationReceipt,
  RequiresApprovalResult,
  RuntimeContractIdentity,
  ServerDecisionPayload,
} from "./types.js";

export type CredentialProvider =
  | { mode: "local-development" }
  | { mode: "bearer"; getToken(): Promise<string> };

export interface StartupRegistrationConfig {
  mode: "startup-register";
  url: string;
  bundle: DecisionDefinitionBundle;
  credential: CredentialProvider;
}

export interface PreRegisteredConfig {
  mode: "pre-registered";
  receipt: RegistrationReceipt;
}

export interface FlaggoClientConfig {
  dataPlaneUrl: string;
  appId: string;
  environment: string;
  deploymentId?: string;
  dataPlaneCredential?: CredentialProvider;
  controlPlane: StartupRegistrationConfig | PreRegisteredConfig;
  availabilityFallback?: {
    mode: "disabled" | "local-default";
    retries?: 0 | 1 | 2;
  };
  fetch?: FetchLike;
}

export interface FlaggoClient {
  readonly tune: {
    number(
      decisionKey: string,
      request: NumberTuneRequest,
    ): Promise<DecisionReceipt<number>>;
    numberDetailed(
      decisionKey: string,
      request: NumberTuneRequest,
    ): Promise<DecisionResult<number>>;
  };
  readonly exposures: {
    confirm(
      decisionId: string,
      confirmToken: string,
      options?: { appliedAt?: string; correlationId?: string },
    ): Promise<ExposureConfirmationResult>;
  };
  readonly definitions: {
    exportBundle(): DecisionDefinitionBundle;
    getRegistrationReceipt(): RegistrationReceipt;
  };
}

async function authorization(
  credential: CredentialProvider | undefined,
): Promise<string | undefined> {
  if (credential === undefined) return undefined;
  if (credential.mode === "local-development") return "Flaggo-Local-Development";
  return `Bearer ${await credential.getToken()}`;
}

function headers(
  authorizationValue: string | undefined,
  correlationId?: string,
): Headers {
  const value = new Headers({ "Content-Type": "application/json" });
  if (authorizationValue !== undefined) {
    value.set("Authorization", authorizationValue);
  }
  if (correlationId !== undefined) {
    value.set("X-Flaggo-Correlation-Id", correlationId);
  }
  return value;
}

async function parseJson(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch (error) {
    throw new InvalidServerResponseError(
      `Flaggo returned non-JSON HTTP ${response.status}.`,
      { cause: error },
    );
  }
}

function problemOrThrow(body: unknown, status: number): ProblemDetails {
  if (!isProblem(body) || body.status !== status) {
    throw new InvalidServerResponseError(
      `Flaggo returned malformed Problem Details for HTTP ${status}.`,
    );
  }
  return body;
}

async function register(
  config: StartupRegistrationConfig,
  fetch: FetchLike,
): Promise<RegistrationReceipt> {
  const bundle = normalizeBundle(config.bundle);
  const digest = bundleDigest(config.bundle);
  const auth = await authorization(config.credential);
  const response = await fetch(
    `${config.url.replace(/\/$/, "")}/v1/definition-bundles:apply`,
    {
      method: "POST",
      headers: {
        ...Object.fromEntries(headers(auth)),
        "Idempotency-Key": `flaggo:${bundle.application.id}:${bundle.application.environment}:${digest}`,
      },
      body: JSON.stringify(bundle),
    },
  );
  const body = await parseJson(response);
  if (response.status === 202) {
    if (!isRequiresApprovalResult(body, bundle, digest)) {
      throw new InvalidServerResponseError(
        "Flaggo returned a malformed requires-approval result.",
      );
    }
    throw new RequiresApprovalError(body);
  }
  if (!response.ok) {
    throw new FlaggoHttpError(problemOrThrow(body, response.status));
  }
  if (!isRegistrationReceipt(body)) {
    throw new InvalidServerResponseError(
      "Flaggo returned a malformed registration receipt.",
    );
  }
  const receipt = body;
  if (
    receipt.status !== "approved"
    || receipt.bundleDigest !== digest
    || receipt.application !== bundle.application.id
    || receipt.environment !== bundle.application.environment
  ) {
    throw new InvalidServerResponseError(
      "Registration receipt does not match the submitted canonical bundle.",
    );
  }
  return receipt;
}

function expectedIdentity(
  receipt: RegistrationReceipt,
  decisionKey: string,
  accepted: AcceptedDefinition,
  config: FlaggoClientConfig,
): RuntimeContractIdentity {
  return {
    ...accepted,
    bundleDigest: receipt.bundleDigest,
    ...(receipt.buildId === undefined ? {} : { buildId: receipt.buildId }),
    ...(receipt.artifactDigest === undefined
      ? {}
      : { artifactDigest: receipt.artifactDigest }),
    ...(config.deploymentId === undefined
      ? {}
      : { deploymentId: config.deploymentId }),
  };
}

function projectReceipt(
  result: DecisionResult<number>,
): DecisionReceipt<number> {
  if (result.source === "client-fallback") {
    return {
      source: result.source,
      value: result.value,
      expectedContract: result.expectedContract,
      reason: result.reason,
    };
  }
  return {
    source: result.source,
    value: result.value,
    decisionId: result.decisionId,
    exposure: result.exposure,
  };
}

function isProblem(value: unknown): value is ProblemDetails {
  const problem = record(value);
  if (problem === undefined) return false;
  const fallback = problem.clientFallback === undefined
    ? undefined
    : record(problem.clientFallback);
  return (
    isUriReference(problem.type)
    && Number.isInteger(problem.status)
    && Number(problem.status) >= 100
    && Number(problem.status) <= 599
    && typeof problem.code === "string"
    && /^[a-z][a-z0-9-]*$/.test(problem.code)
    && (problem.title === undefined || typeof problem.title === "string")
    && (problem.detail === undefined || typeof problem.detail === "string")
    && (problem.instance === undefined || isUriReference(problem.instance))
    && (
      problem.correlationId === undefined
      || typeof problem.correlationId === "string"
    )
    && (
      problem.retryAfterSeconds === undefined
      || (
        Number.isInteger(problem.retryAfterSeconds)
        && Number(problem.retryAfterSeconds) >= 0
      )
    )
    && (
      problem.clientFallback === undefined
      || (
        fallback !== undefined
        && Object.keys(fallback).every(
          (key) => key === "eligible" || key === "reason",
        )
        && typeof fallback.eligible === "boolean"
        && (fallback.reason === undefined || typeof fallback.reason === "string")
      )
    )
    && (
      problem.issues === undefined
      || (
        Array.isArray(problem.issues)
        && problem.issues.every(isProblemIssue)
      )
    )
  );
}

function isUriReference(value: unknown): value is string {
  if (typeof value !== "string") return false;
  if (!/^[\x21-\x7e]*$/.test(value) || /%(?![0-9a-f]{2})/i.test(value)) {
    return false;
  }
  return URL.canParse(value, "https://flaggo.invalid/");
}

function isProblemIssue(value: unknown): boolean {
  const issue = record(value);
  if (issue === undefined) return false;
  const allowedKeys = new Set([
    "code",
    "severity",
    "path",
    "message",
    "decisionKey",
    "signalKey",
  ]);
  return Object.keys(issue).every((key) => allowedKeys.has(key))
    && typeof issue.code === "string"
    && /^[a-z][a-z0-9-]*$/.test(issue.code)
    && (issue.severity === "error" || issue.severity === "warning")
    && typeof issue.path === "string"
    && typeof issue.message === "string"
    && (issue.decisionKey === undefined || typeof issue.decisionKey === "string")
    && (issue.signalKey === undefined || typeof issue.signalKey === "string");
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function hasOnlyKeys(
  value: Record<string, unknown>,
  allowed: readonly string[],
): boolean {
  const allowedKeys = new Set(allowed);
  return Object.keys(value).every((key) => allowedKeys.has(key));
}

function hasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function stringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isDigest(value: unknown): value is `sha256:${string}` {
  return typeof value === "string" && /^sha256:[0-9a-f]{64}$/.test(value);
}

function isAcceptedDefinition(value: unknown): value is AcceptedDefinition {
  const accepted = record(value);
  return accepted !== undefined
    && hasOnlyKeys(accepted, ["definitionId", "revision", "contractDigest"])
    && nonEmptyString(accepted.definitionId)
    && nonEmptyString(accepted.revision)
    && isDigest(accepted.contractDigest);
}

function isProposedDefinition(value: unknown): boolean {
  const proposed = record(value);
  return proposed !== undefined
    && hasOnlyKeys(proposed, ["definitionId", "contractDigest"])
    && nonEmptyString(proposed.definitionId)
    && isDigest(proposed.contractDigest);
}

function isCanonicalJsonValue(value: unknown): boolean {
  if (
    value === null
    || typeof value === "boolean"
    || typeof value === "string"
  ) return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value)) return value.every(isCanonicalJsonValue);
  const object = record(value);
  return object !== undefined && Object.values(object).every(isCanonicalJsonValue);
}

function isSemanticDiffOperation(value: unknown): boolean {
  const operation = record(value);
  if (operation === undefined || typeof operation.path !== "string") return false;
  if (operation.op === "add") {
    return hasOnlyKeys(operation, ["op", "path", "after"])
      && hasOwn(operation, "after")
      && isCanonicalJsonValue(operation.after);
  }
  if (operation.op === "remove") {
    return hasOnlyKeys(operation, ["op", "path", "before"])
      && hasOwn(operation, "before")
      && isCanonicalJsonValue(operation.before);
  }
  return operation.op === "replace"
    && hasOnlyKeys(operation, ["op", "path", "before", "after"])
    && hasOwn(operation, "before")
    && hasOwn(operation, "after")
    && isCanonicalJsonValue(operation.before)
    && isCanonicalJsonValue(operation.after);
}

function isContractChange(value: unknown): boolean {
  const change = record(value);
  if (change === undefined || !nonEmptyString(change.decisionKey)) return false;
  if (
    change.kind === "created"
    || change.kind === "metadata-updated"
    || change.kind === "deprecation-candidate"
  ) {
    return hasOnlyKeys(change, ["kind", "decisionKey"]);
  }
  return change.kind === "semantic-change"
    && hasOnlyKeys(
      change,
      ["kind", "decisionKey", "previous", "proposed", "semanticDiff"],
    )
    && isAcceptedDefinition(change.previous)
    && isProposedDefinition(change.proposed)
    && Array.isArray(change.semanticDiff)
    && change.semanticDiff.length > 0
    && change.semanticDiff.every(isSemanticDiffOperation);
}

function isContractIssue(value: unknown, warningOnly = false): boolean {
  const issue = record(value);
  if (issue === undefined) return false;
  return hasOnlyKeys(
    issue,
    ["code", "severity", "path", "message", "decisionKey", "signalKey"],
  )
    && typeof issue.code === "string"
    && /^[a-z][a-z0-9-]*$/.test(issue.code)
    && (
      issue.severity === "warning"
      || (!warningOnly && issue.severity === "error")
    )
    && typeof issue.path === "string"
    && typeof issue.message === "string"
    && (issue.decisionKey === undefined || typeof issue.decisionKey === "string")
    && (issue.signalKey === undefined || typeof issue.signalKey === "string");
}

function isRegistrationReceipt(value: unknown): value is RegistrationReceipt {
  const receipt = record(value);
  const accepted = record(receipt?.acceptedDefinitions);
  return receipt !== undefined
    && hasOnlyKeys(receipt, [
      "application",
      "environment",
      "bundleDigest",
      "buildId",
      "artifactDigest",
      "acceptedDefinitions",
      "compatibility",
      "status",
      "changes",
      "issues",
    ])
    && nonEmptyString(receipt.application)
    && nonEmptyString(receipt.environment)
    && isDigest(receipt.bundleDigest)
    && (receipt.buildId === undefined || typeof receipt.buildId === "string")
    && (
      receipt.artifactDigest === undefined
      || typeof receipt.artifactDigest === "string"
    )
    && accepted !== undefined
    && Object.keys(accepted).length > 0
    && Object.values(accepted).every(isAcceptedDefinition)
    && ["identical", "metadata-only", "new-contract-required"].includes(
      String(receipt.compatibility),
    )
    && receipt.status === "approved"
    && (
      receipt.changes === undefined
      || (
        Array.isArray(receipt.changes)
        && receipt.changes.every(isContractChange)
      )
    )
    && Array.isArray(receipt.issues)
    && receipt.issues.every((issue) => isContractIssue(issue, true));
}

function isRequiresApprovalResult(
  value: unknown,
  bundle: DecisionDefinitionBundle,
  digest: `sha256:${string}`,
): value is RequiresApprovalResult {
  const result = record(value);
  return result !== undefined
    && hasOnlyKeys(result, [
      "status",
      "approvalRequestId",
      "application",
      "environment",
      "bundleDigest",
      "compatibility",
      "expiresAt",
      "snapshotUrl",
      "supersedesApprovalRequestId",
      "changes",
      "issues",
    ])
    && result.status === "requires-approval"
    && nonEmptyString(result.approvalRequestId)
    && result.application === bundle.application.id
    && result.environment === bundle.application.environment
    && result.bundleDigest === digest
    && result.compatibility === "new-contract-required"
    && isRfc3339Utc(result.expiresAt)
    && typeof result.snapshotUrl === "string"
    && (
      result.supersedesApprovalRequestId === undefined
      || typeof result.supersedesApprovalRequestId === "string"
    )
    && Array.isArray(result.changes)
    && result.changes.length > 0
    && result.changes.every(isContractChange)
    && result.changes.some(
      (change) => record(change)?.kind === "semantic-change",
    )
    && Array.isArray(result.issues)
    && result.issues.every((issue) => isContractIssue(issue, true));
}

function isTarget(value: unknown): boolean {
  const target = record(value);
  return target !== undefined
    && hasOnlyKeys(target, ["type", "id"])
    && nonEmptyString(target.type)
    && nonEmptyString(target.id);
}

function isTargetProvenance(value: unknown): boolean {
  const provenance = record(value);
  return provenance !== undefined
    && hasOnlyKeys(
      provenance,
      ["targetType", "claimedId", "resolvedId", "source"],
    )
    && nonEmptyString(provenance.targetType)
    && (
      provenance.claimedId === undefined
      || typeof provenance.claimedId === "string"
    )
    && nonEmptyString(provenance.resolvedId)
    && [
      "client-claimed",
      "client-verified",
      "server-derived",
      "server-replaced",
    ].includes(String(provenance.source));
}

function isExposure(value: unknown): boolean {
  const exposure = record(value);
  if (exposure === undefined) return false;
  const keys = Object.keys(exposure);
  if (exposure.confirmationRequired === false) {
    return keys.length === 1 && keys[0] === "confirmationRequired";
  }
  return exposure.confirmationRequired === true
    && keys.length === 2
    && keys.includes("confirmationRequired")
    && keys.includes("confirmToken")
    && nonEmptyString(exposure.confirmToken);
}

function isConfidence(value: unknown): boolean {
  if (value === null) return true;
  const confidence = record(value);
  const probability = (candidate: unknown): boolean =>
    typeof candidate === "number"
    && Number.isFinite(candidate)
    && candidate >= 0
    && candidate <= 1;
  return confidence !== undefined
    && hasOnlyKeys(
      confidence,
      ["evidenceQuality", "modelUncertainty", "expectedOutcome"],
    )
    && probability(confidence.evidenceQuality)
    && (
      confidence.modelUncertainty === undefined
      || probability(confidence.modelUncertainty)
    )
    && (
      confidence.expectedOutcome === undefined
      || probability(confidence.expectedOutcome)
    );
}

function isPolicyResult(value: unknown): boolean {
  const policy = record(value);
  if (policy === undefined) return false;
  const clientFallback = policy.clientFallback === undefined
    ? undefined
    : record(policy.clientFallback);
  return hasOnlyKeys(
    policy,
    ["result", "reasons", "appliedConstraints", "clientFallback"],
  )
    && ["approved", "blocked", "fallback"].includes(String(policy.result))
    && stringArray(policy.reasons)
    && stringArray(policy.appliedConstraints)
    && (
      policy.clientFallback === undefined
      || (
        clientFallback !== undefined
        && hasOnlyKeys(clientFallback, ["requiredEvidenceUnavailable"])
        && (
          clientFallback.requiredEvidenceUnavailable === "allow"
          || clientFallback.requiredEvidenceUnavailable === "forbid"
        )
      )
    );
}

function isVerifiedNumberResult(
  value: unknown,
  decisionKey: string,
  expected: RuntimeContractIdentity,
  appId: string,
  environment: string,
): value is ServerDecisionPayload<number> {
  if (value === null || typeof value !== "object") return false;
  const result = value as Record<string, unknown>;
  const definitionStatus = record(result.definitionStatus);
  const definition = record(result.definition);
  const fallback = record(result.fallback);
  const policy = record(result.policy);
  if (
    definitionStatus === undefined
    || definition === undefined
    || fallback === undefined
    || policy === undefined
  ) return false;
  if (!hasOnlyKeys(result, [
    "decisionKey",
    "definition",
    "decisionId",
    "value",
    "valueType",
    "decisionMode",
    "strategyId",
    "confidence",
    "runtimeTarget",
    "controlTarget",
    "targetProvenance",
    "resolutionChain",
    "fallback",
    "policy",
    "definitionStatus",
    "exposure",
    "reason",
    "auditId",
  ])) return false;
  const mode = result.decisionMode;
  const modeValid = (
    mode === "active-value"
    && result.strategyId === undefined
    && fallback.decisionFallbackUsed === false
    && policy.result === "approved"
  ) || (
    (mode === "strategy" || mode === "experiment")
    && nonEmptyString(result.strategyId)
    && result.confidence !== null
    && fallback.decisionFallbackUsed === false
    && policy.result === "approved"
  ) || (
    mode === "fallback"
    && result.strategyId === undefined
    && result.confidence === null
    && fallback.decisionFallbackUsed === true
    && (policy.result === "blocked" || policy.result === "fallback")
  );
  return (
    result.decisionKey === decisionKey
    && hasOnlyKeys(
      definition,
      ["appId", "environment", "key", "definitionId", "revision"],
    )
    && definition.key === decisionKey
    && definition.appId === appId
    && definition.environment === environment
    && definition.definitionId === expected.definitionId
    && definition.revision === expected.revision
    && nonEmptyString(result.decisionId)
    && result.valueType === "number"
    && typeof result.value === "number"
    && modeValid
    && isConfidence(result.confidence)
    && (result.runtimeTarget === undefined || isTarget(result.runtimeTarget))
    && (result.controlTarget === undefined || isTarget(result.controlTarget))
    && Array.isArray(result.targetProvenance)
    && result.targetProvenance.every(isTargetProvenance)
    && Array.isArray(result.resolutionChain)
    && stringArray(result.resolutionChain)
    && hasOnlyKeys(
      fallback,
      [
        "source",
        "resolutionFallbackUsed",
        "decisionFallbackUsed",
        "reason",
      ],
    )
    && fallback.source === "server"
    && typeof fallback.resolutionFallbackUsed === "boolean"
    && typeof fallback.decisionFallbackUsed === "boolean"
    && (fallback.reason === null || typeof fallback.reason === "string")
    && isPolicyResult(policy)
    && hasOnlyKeys(definitionStatus, [
      "definitionId",
      "revision",
      "contractDigest",
      "bundleDigest",
      "buildId",
      "deploymentId",
      "integrity",
      "compatibility",
    ])
    && definitionStatus.definitionId === expected.definitionId
    && definitionStatus.revision === expected.revision
    && definitionStatus.contractDigest === expected.contractDigest
    && definitionStatus.integrity === "verified"
    && (
      definitionStatus.bundleDigest === undefined
      || isDigest(definitionStatus.bundleDigest)
    )
    && (
      definitionStatus.buildId === undefined
      || typeof definitionStatus.buildId === "string"
    )
    && (
      definitionStatus.deploymentId === undefined
      || typeof definitionStatus.deploymentId === "string"
    )
    && (
      expected.bundleDigest === undefined
      || definitionStatus.bundleDigest === expected.bundleDigest
    )
    && (
      expected.buildId === undefined
      || definitionStatus.buildId === expected.buildId
    )
    && (
      expected.deploymentId === undefined
      || definitionStatus.deploymentId === expected.deploymentId
    )
    && (
      definitionStatus.compatibility === undefined
      || [
        "identical",
        "metadata-only",
        "new-contract-required",
      ].includes(String(definitionStatus.compatibility))
    )
    && isExposure(result.exposure)
    && typeof result.reason === "string"
    && nonEmptyString(result.auditId)
  );
}

function isExposureConfirmationResult(
  value: unknown,
  decisionId: string,
): value is ExposureConfirmationResult {
  const result = record(value);
  return result !== undefined
    && hasOnlyKeys(
      result,
      ["exposureId", "decisionId", "status", "confirmedAt"],
    )
    && nonEmptyString(result.exposureId)
    && result.decisionId === decisionId
    && result.status === "confirmed"
    && isRfc3339Utc(result.confirmedAt);
}

function isRfc3339Utc(value: unknown): value is string {
  if (typeof value !== "string") return false;
  const match = /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/
    .exec(value);
  if (match === null) return false;
  const [, yearText, monthText, dayText, hourText, minuteText, secondText] =
    match;
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  const hour = Number(hourText);
  const minute = Number(minuteText);
  const second = Number(secondText);
  if (
    year < 1
    || month < 1
    || month > 12
    || day < 1
    || hour > 23
    || minute > 59
    || second > 59
  ) return false;
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const daysInMonth = [
    31,
    leapYear ? 29 : 28,
    31,
    30,
    31,
    30,
    31,
    31,
    30,
    31,
    30,
    31,
  ][month - 1]!;
  return day <= daysInMonth;
}

const RETRYABLE_TRANSPORT_CODES = new Set([
  "EAI_AGAIN",
  "ECONNREFUSED",
  "ECONNRESET",
  "ENOTFOUND",
  "ETIMEDOUT",
  "UND_ERR_BODY_TIMEOUT",
  "UND_ERR_CONNECT_TIMEOUT",
  "UND_ERR_HEADERS_TIMEOUT",
  "UND_ERR_SOCKET",
]);

function isRetryableTransportFailure(error: unknown): boolean {
  let current = error;
  const visited = new Set<unknown>();
  while (
    current !== null
    && typeof current === "object"
    && !visited.has(current)
  ) {
    visited.add(current);
    const candidate = current as {
      name?: unknown;
      code?: unknown;
      cause?: unknown;
    };
    if (candidate.name === "AbortError") return false;
    if (candidate.name === "TimeoutError") return true;
    if (
      typeof candidate.code === "string"
      && RETRYABLE_TRANSPORT_CODES.has(candidate.code)
    ) {
      return true;
    }
    current = candidate.cause;
  }
  return false;
}

function errorChainHasName(error: unknown, name: string): boolean {
  return findErrorInChain(error, name) !== undefined;
}

function findErrorInChain(error: unknown, name: string): unknown {
  let current = error;
  const visited = new Set<unknown>();
  while (
    current !== null
    && typeof current === "object"
    && !visited.has(current)
  ) {
    visited.add(current);
    const candidate = current as { name?: unknown; cause?: unknown };
    if (candidate.name === name) return current;
    current = candidate.cause;
  }
  return undefined;
}

function retryDelayMilliseconds(response?: Response): number {
  const retryAfter = response?.headers.get("Retry-After");
  if (retryAfter !== null && retryAfter !== undefined) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds) && seconds >= 0) {
      return Math.min(seconds * 1_000, 1_000);
    }
    const date = Date.parse(retryAfter);
    if (Number.isFinite(date)) {
      return Math.min(Math.max(date - Date.now(), 0), 1_000);
    }
  }
  return 50 + Math.floor(Math.random() * 101);
}

async function waitBeforeRetry(response?: Response): Promise<void> {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, retryDelayMilliseconds(response));
  });
}

export async function createFlaggoClient(
  config: FlaggoClientConfig,
): Promise<FlaggoClient> {
  const fetch = config.fetch ?? globalThis.fetch.bind(globalThis);
  const receipt = config.controlPlane.mode === "startup-register"
    ? await register(config.controlPlane, fetch)
    : config.controlPlane.receipt;
  if (
    !isRegistrationReceipt(receipt)
    || receipt.application !== config.appId
    || receipt.environment !== config.environment
  ) {
    throw new InvalidServerResponseError(
      "Registration receipt does not match the configured application identity.",
    );
  }
  const bundle = config.controlPlane.mode === "startup-register"
    ? normalizeBundle(config.controlPlane.bundle)
    : undefined;
  const definitions = new Map(
    bundle?.definitions.map((definition) => [definition.key, definition]),
  );

  async function numberDetailed(
    decisionKey: string,
    request: NumberTuneRequest,
  ): Promise<DecisionResult<number>> {
    const accepted = receipt.acceptedDefinitions[decisionKey];
    if (accepted === undefined) {
      throw new MissingAcceptedDefinitionError(decisionKey);
    }
    const actualDigest = contractDigest(request.definition);
    if (actualDigest !== accepted.contractDigest) {
      throw new ContractConflictError(
        decisionKey,
        accepted.contractDigest,
        actualDigest,
      );
    }
    const startupDefinition = definitions.get(decisionKey);
    if (
      startupDefinition !== undefined
      && contractDigest(startupDefinition) !== actualDigest
    ) {
      throw new ContractConflictError(
        decisionKey,
        contractDigest(startupDefinition),
        actualDigest,
      );
    }
    const expectedContract = expectedIdentity(
      receipt,
      decisionKey,
      accepted,
      config,
    );
    const auth = await authorization(config.dataPlaneCredential);
    const fallbackEnabled =
      config.availabilityFallback?.mode === "local-default";
    const retries = fallbackEnabled
      ? config.availabilityFallback?.retries ?? 1
      : 0;
    const idempotencyKey = request.idempotencyKey
      ?? (retries > 0 ? `flaggo-sdk:${randomUUID()}` : undefined);
    const requestBody = JSON.stringify({
      expectedContract,
      ...(request.runtimeTarget === undefined
        ? {}
        : { runtimeTarget: request.runtimeTarget }),
      runtimeContext: request.context,
      ...(request.inputs === undefined
        ? {}
        : {
            inputs: [...request.inputs].sort((left, right) =>
              compareCanonicalStrings(left.signal.key, right.signal.key)
            ),
          }),
      client: {
        appId: config.appId,
        environment: config.environment,
        sdk: "typescript",
        sdkVersion: "0.1.0",
      },
    });
    const requestInit: RequestInit = {
      method: "POST",
      headers: {
        ...Object.fromEntries(headers(auth, request.correlationId)),
        ...(idempotencyKey === undefined
          ? {}
          : { "Idempotency-Key": idempotencyKey }),
      },
      body: requestBody,
    };
    const url =
      `${config.dataPlaneUrl.replace(/\/$/, "")}/v1/decisions/${encodeURIComponent(decisionKey)}:decide`;

    for (let attempt = 0; attempt <= retries; attempt += 1) {
      let response: Response;
      try {
        response = await fetch(url, requestInit);
      } catch (error) {
        if (!fallbackEnabled || !isRetryableTransportFailure(error)) throw error;
        if (attempt < retries) {
          await waitBeforeRetry();
          continue;
        }
        return localFallback(
          decisionKey,
          request.definition.actionSpace.default,
          expectedContract,
          "data-plane transport failure",
        );
      }

      const intermediaryStatus =
        response.status === 502 || response.status === 504;
      const problemContentType = response.headers
        .get("Content-Type")
        ?.toLowerCase()
        .includes("application/problem+json") === true;
      let body: unknown;
      let bodyParsed = false;
      if (intermediaryStatus && problemContentType) {
        try {
          body = await parseJson(response);
          bodyParsed = true;
        } catch (error) {
          const genericIntermediaryBody =
            errorChainHasName(error, "SyntaxError");
          const cancellation = findErrorInChain(error, "AbortError");
          if (cancellation !== undefined) throw cancellation;
          if (
            !fallbackEnabled
            || (
              !isRetryableTransportFailure(error)
              && !genericIntermediaryBody
            )
          ) {
            throw error;
          }
          if (attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          return localFallback(
            decisionKey,
            request.definition.actionSpace.default,
            expectedContract,
            `intermediary HTTP ${response.status}`,
          );
        }
        const genericProblem = record(body);
        if (
          genericProblem !== undefined
          && hasOwn(genericProblem, "status")
          && (
            !Number.isInteger(genericProblem.status)
            || genericProblem.status !== response.status
          )
        ) {
          throw new InvalidServerResponseError(
            `Problem Details status does not match HTTP ${response.status}.`,
          );
        }
      }
      if (
        intermediaryStatus
        && (!problemContentType || !isProblem(body))
      ) {
        if (!fallbackEnabled) {
          if (!bodyParsed) body = await parseJson(response);
          throw new InvalidServerResponseError(
            `Intermediary HTTP ${response.status} is not Flaggo Problem Details.`,
          );
        }
        if (attempt < retries) {
          await waitBeforeRetry(response);
          continue;
        }
        return localFallback(
          decisionKey,
          request.definition.actionSpace.default,
          expectedContract,
          `intermediary HTTP ${response.status}`,
        );
      }

      if (!bodyParsed) {
        try {
          body = await parseJson(response);
        } catch (error) {
          const cancellation = findErrorInChain(error, "AbortError");
          if (cancellation !== undefined) throw cancellation;
          if (
            !fallbackEnabled
            || !response.ok
            || !isRetryableTransportFailure(error)
          ) {
            throw error;
          }
          if (attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          return localFallback(
            decisionKey,
            request.definition.actionSpace.default,
            expectedContract,
            "data-plane response transport failure",
          );
        }
      }
      if (!response.ok) {
        const problem = problemOrThrow(body, response.status);
        if (
          (response.status === 502
            || response.status === 503
            || response.status === 504)
          && problem.clientFallback?.eligible === true
        ) {
          if (fallbackEnabled && attempt < retries) {
            await waitBeforeRetry(response);
            continue;
          }
          if (fallbackEnabled) {
            return localFallback(
              decisionKey,
              request.definition.actionSpace.default,
              expectedContract,
              problem.clientFallback.reason ?? problem.code,
            );
          }
        }
        throw new FlaggoHttpError(problem);
      }
      if (
        !isVerifiedNumberResult(
          body,
          decisionKey,
          expectedContract,
          config.appId,
          config.environment,
        )
      ) {
        throw new InvalidServerResponseError(
          `Decision '${decisionKey}' returned an invalid or unverified result.`,
        );
      }
      return { ...body, source: "server" };
    }
    throw new Error("Unreachable retry state.");
  }

  return {
    tune: {
      async number(decisionKey, request) {
        return projectReceipt(await numberDetailed(decisionKey, request));
      },
      numberDetailed,
    },
    exposures: {
      async confirm(decisionId, confirmToken, options = {}) {
        const auth = await authorization(config.dataPlaneCredential);
        const response = await fetch(
          `${config.dataPlaneUrl.replace(/\/$/, "")}/v1/exposures/${encodeURIComponent(decisionId)}:confirm`,
          {
            method: "POST",
            headers: headers(auth, options.correlationId),
            body: JSON.stringify({
              confirmToken,
              ...(options.appliedAt === undefined
                ? {}
                : { appliedAt: options.appliedAt }),
            }),
          },
        );
        const body = await parseJson(response);
        if (!response.ok) {
          throw new FlaggoHttpError(problemOrThrow(body, response.status));
        }
        if (!isExposureConfirmationResult(body, decisionId)) {
          throw new InvalidServerResponseError(
            "Flaggo returned a malformed exposure confirmation.",
          );
        }
        return body;
      },
    },
    definitions: {
      exportBundle() {
        if (bundle === undefined) {
          throw new Error("No code-first bundle was supplied to this client.");
        }
        return structuredClone(bundle);
      },
      getRegistrationReceipt() {
        return structuredClone(receipt);
      },
    },
  };
}

function localFallback(
  decisionKey: string,
  value: number,
  expectedContract: RuntimeContractIdentity,
  reason: string,
): ClientFallbackResult<number> {
  return {
    source: "client-fallback",
    decisionKey,
    expectedContract,
    value,
    valueType: "number",
    decisionMode: "fallback",
    confidence: null,
    reason,
    fallback: {
      source: "client-fallback",
      resolutionFallbackUsed: false,
      decisionFallbackUsed: true,
      reason,
    },
  };
}
