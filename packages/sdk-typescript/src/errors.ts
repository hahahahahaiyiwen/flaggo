import type {
  FlaggoResponseMetadata,
  ProblemDetails,
  ValidationIssue,
} from "./types.js";

export class FlaggoError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = new.target.name;
  }
}

export class MissingContractBindingError extends FlaggoError {
  constructor(readonly contractName: string) {
    super(`No runtime contract binding exists for '${contractName}'.`);
  }
}

export class InvalidDecisionInputError extends FlaggoError {
  readonly issues: ValidationIssue[];

  constructor(path: string, message: string, code = "invalid-runtime-input") {
    super(`${path}: ${message}`);
    this.issues = [{ code, severity: "error", path, message }];
  }
}

export class FlaggoHttpError extends FlaggoError {
  constructor(
    readonly problem: ProblemDetails,
    readonly response: FlaggoResponseMetadata = {},
  ) {
    super(`${problem.type}: ${problem.detail ?? problem.title ?? "Flaggo request failed"}`);
  }
}

export class InvalidServerResponseError extends FlaggoError {}
