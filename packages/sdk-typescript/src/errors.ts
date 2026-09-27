import type {
  FlaggoResponseMetadata,
  ProblemDetails,
  SdkValidationIssue,
} from "./shared/types.js";

export class FlaggoError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = new.target.name;
  }
}

export class MissingDecisionBindingError extends FlaggoError {
  constructor(readonly contractName: string) {
    super(`No decision binding exists for '${contractName}'.`);
  }
}

export class InvalidFlaggoInputError extends FlaggoError {
  readonly issues: readonly SdkValidationIssue[];

  constructor(
    message: string,
    issues: readonly SdkValidationIssue[] = [],
    options?: ErrorOptions,
  ) {
    super(message, options);
    this.issues = issues;
  }
}

export class FlaggoHttpError extends FlaggoError {
  constructor(
    readonly problem: ProblemDetails,
    readonly response: FlaggoResponseMetadata,
  ) {
    super(`${problem.type}: ${problem.detail ?? problem.title ?? "Flaggo request failed"}`);
  }
}

export class FlaggoTransportError extends FlaggoError {}

export class FlaggoTimeoutError extends FlaggoTransportError {
  constructor(readonly timeoutMs: number, options?: ErrorOptions) {
    super(`Flaggo request timed out after ${timeoutMs} ms.`, options);
  }
}

export class FlaggoAbortError extends FlaggoTransportError {
  constructor(options?: ErrorOptions) {
    super("Flaggo request was aborted.", options);
  }
}

export class InvalidServerResponseError extends FlaggoError {
  constructor(
    message: string,
    readonly issues: readonly SdkValidationIssue[] = [],
    options?: ErrorOptions,
  ) {
    super(message, options);
  }
}
