import type {
  ProblemDetails,
  RequiresApprovalResult,
  Sha256Digest,
} from "./types.js";

export class FlaggoError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = new.target.name;
  }
}

export class RequiresApprovalError extends FlaggoError {
  readonly approvalRequestId: string;
  readonly bundleDigest: Sha256Digest;
  readonly expiresAt: string;
  readonly snapshotUrl: string;

  constructor(result: RequiresApprovalResult) {
    super(`Definition bundle requires approval: ${result.approvalRequestId}`);
    this.approvalRequestId = result.approvalRequestId;
    this.bundleDigest = result.bundleDigest;
    this.expiresAt = result.expiresAt;
    this.snapshotUrl = result.snapshotUrl;
  }
}

export class MissingAcceptedDefinitionError extends FlaggoError {
  constructor(readonly decisionKey: string) {
    super(`No accepted runtime binding exists for '${decisionKey}'.`);
  }
}

export class MissingStaticDefinitionError extends FlaggoError {
  constructor(readonly decisionKey: string) {
    super(
      `No static definition exists for '${decisionKey}'. `
        + "Supply its extracted bundle at startup or pass definition explicitly.",
    );
  }
}

export class ContractConflictError extends FlaggoError {
  constructor(
    readonly decisionKey: string,
    readonly expectedDigest: Sha256Digest,
    readonly actualDigest: Sha256Digest,
  ) {
    super(
      `Definition '${decisionKey}' hashes to ${actualDigest}, `
      + `but startup accepted ${expectedDigest}.`,
    );
  }
}

export class FlaggoHttpError extends FlaggoError {
  constructor(readonly problem: ProblemDetails) {
    super(`${problem.code}: ${problem.detail ?? problem.title ?? "Flaggo request failed"}`);
  }
}

export class InvalidServerResponseError extends FlaggoError {}
