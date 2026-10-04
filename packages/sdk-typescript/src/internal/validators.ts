import {
  InvalidFlaggoInputError,
  InvalidServerResponseError,
} from "../errors.js";
import type { SdkValidationIssue } from "../shared/types.js";

interface SchemaValidationError {
  readonly instancePath: string;
  readonly keyword: string;
  readonly message?: string;
}

export interface StandaloneValidator {
  (value: unknown): boolean;
  readonly errors: readonly SchemaValidationError[] | null;
}

function issues(
  errors: readonly SchemaValidationError[] | null,
): readonly SdkValidationIssue[] {
  return (errors ?? []).map((error) => ({
    path: error.instancePath,
    message: error.message ?? `Failed '${error.keyword}' validation.`,
    keyword: error.keyword,
  }));
}

export function assertInputSchema(
  validator: StandaloneValidator,
  value: unknown,
  description: string,
): void {
  if (!validator(value)) {
    throw new InvalidFlaggoInputError(
      `${description} does not satisfy its Flaggo schema.`,
      issues(validator.errors),
    );
  }
}

export function assertResponseSchema(
  validator: StandaloneValidator,
  value: unknown,
  description: string,
): void {
  if (!validator(value)) {
    throw new InvalidServerResponseError(
      `Flaggo returned malformed ${description}.`,
      issues(validator.errors),
    );
  }
}
