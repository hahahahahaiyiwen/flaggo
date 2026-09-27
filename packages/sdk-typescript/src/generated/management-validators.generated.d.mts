/*
 * Generated from Flaggo v3 JSON Schemas. Do not edit by hand.
 * Run `npm run generate --workspace @flaggo/sdk` after schema changes.
 */

export interface SchemaValidationError {
  readonly instancePath: string;
  readonly schemaPath: string;
  readonly keyword: string;
  readonly params: Readonly<Record<string, unknown>>;
  readonly message?: string;
}

export interface StandaloneValidator {
  (value: unknown): boolean;
  errors: readonly SchemaValidationError[] | null;
}

export const validateDecisionContract: StandaloneValidator;
export const validateDecisionContractValidationResult: StandaloneValidator;
export const validateDecisionContractVersion: StandaloneValidator;
export const validateDecisionContractVersionList: StandaloneValidator;
