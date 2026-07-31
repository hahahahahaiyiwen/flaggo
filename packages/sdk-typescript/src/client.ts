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
  ServerDecisionResult,
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
  if (!isProblem(body)) {
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
    if (
      body === null
      || typeof body !== "object"
      || (body as Record<string, unknown>).status !== "requires-approval"
      || typeof (body as Record<string, unknown>).approvalRequestId !== "string"
    ) {
      throw new InvalidServerResponseError(
        "Flaggo returned a malformed requires-approval result.",
      );
    }
    throw new RequiresApprovalError(body as RequiresApprovalResult);
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
  if (receipt.status !== "approved" || receipt.bundleDigest !== digest) {
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
  if (value === null || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.status === "number"
    && typeof record.code === "string"
    && typeof record.detail === "string"
  );
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
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
    && nonEmptyString(accepted.definitionId)
    && nonEmptyString(accepted.revision)
    && isDigest(accepted.contractDigest);
}

function isRegistrationReceipt(value: unknown): value is RegistrationReceipt {
  const receipt = record(value);
  const accepted = record(receipt?.acceptedDefinitions);
  return receipt !== undefined
    && nonEmptyString(receipt.application)
    && nonEmptyString(receipt.environment)
    && isDigest(receipt.bundleDigest)
    && accepted !== undefined
    && Object.keys(accepted).length > 0
    && Object.values(accepted).every(isAcceptedDefinition)
    && ["identical", "metadata-only", "new-contract-required"].includes(
      String(receipt.compatibility),
    )
    && receipt.status === "approved"
    && Array.isArray(receipt.issues);
}

function isTarget(value: unknown): boolean {
  const target = record(value);
  return target !== undefined
    && nonEmptyString(target.type)
    && nonEmptyString(target.id);
}

function isTargetProvenance(value: unknown): boolean {
  const provenance = record(value);
  return provenance !== undefined
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
  return exposure !== undefined
    && (
      exposure.confirmationRequired === false
      || (
        exposure.confirmationRequired === true
        && nonEmptyString(exposure.confirmToken)
      )
    );
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

function isVerifiedNumberResult(
  value: unknown,
  decisionKey: string,
  expected: RuntimeContractIdentity,
  appId: string,
  environment: string,
): value is Omit<ServerDecisionResult<number>, "source"> {
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
    && fallback.source === "server"
    && typeof fallback.resolutionFallbackUsed === "boolean"
    && typeof fallback.decisionFallbackUsed === "boolean"
    && (fallback.reason === null || typeof fallback.reason === "string")
    && ["approved", "blocked", "fallback"].includes(String(policy.result))
    && stringArray(policy.reasons)
    && stringArray(policy.appliedConstraints)
    && definitionStatus.definitionId === expected.definitionId
    && definitionStatus.revision === expected.revision
    && definitionStatus.contractDigest === expected.contractDigest
    && definitionStatus.integrity === "verified"
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
    && nonEmptyString(result.exposureId)
    && result.decisionId === decisionId
    && result.status === "confirmed"
    && typeof result.confirmedAt === "string";
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
    let response: Response;
    try {
      response = await fetch(
        `${config.dataPlaneUrl.replace(/\/$/, "")}/v1/decisions/${encodeURIComponent(decisionKey)}:decide`,
        {
          method: "POST",
          headers: {
            ...Object.fromEntries(headers(auth, request.correlationId)),
            ...(request.idempotencyKey === undefined
              ? {}
              : { "Idempotency-Key": request.idempotencyKey }),
          },
          body: JSON.stringify({
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
          }),
        },
      );
    } catch (error) {
      if (config.availabilityFallback?.mode !== "local-default") throw error;
      return localFallback(
        decisionKey,
        request.definition.actionSpace.default,
        expectedContract,
        "data-plane transport failure",
      );
    }

    const body = await parseJson(response);
    if (!response.ok) {
      if (
        response.status === 503
        && isProblem(body)
        && body.clientFallback?.eligible === true
        && config.availabilityFallback?.mode === "local-default"
      ) {
        return localFallback(
          decisionKey,
          request.definition.actionSpace.default,
          expectedContract,
          body.clientFallback.reason ?? body.code,
        );
      }
      throw new FlaggoHttpError(problemOrThrow(body, response.status));
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
