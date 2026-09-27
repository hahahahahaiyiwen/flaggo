import { InvalidFlaggoInputError } from "../errors.js";
import type { Sha256Digest, SdkValidationIssue } from "../shared/types.js";
import { utf8Length } from "./serialization.js";

const decisionNamePattern = /^[A-Za-z][A-Za-z0-9._-]{0,127}$/u;
const digestPattern = /^sha256:[0-9a-f]{64}$/u;

export function record(
  value: unknown,
): Readonly<Record<string, unknown>> | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null
    ? value as Readonly<Record<string, unknown>>
    : undefined;
}

export function hasOnlyKeys(
  value: Readonly<Record<string, unknown>>,
  allowed: ReadonlySet<string>,
): boolean {
  return Object.keys(value).every((key) => allowed.has(key));
}

export function inputError(path: string, message: string): never {
  const issue: SdkValidationIssue = { path, message };
  throw new InvalidFlaggoInputError(`${path || "/"}: ${message}`, [issue]);
}

export function assertDecisionName(
  value: unknown,
  path = "/contractName",
): asserts value is string {
  if (typeof value !== "string" || !decisionNamePattern.test(value)) {
    inputError(
      path,
      "Decision names must begin with a letter and contain at most 128 letters, digits, dots, underscores, or hyphens.",
    );
  }
}

export function assertSha256Digest(
  value: unknown,
  path = "/contractDigest",
): asserts value is Sha256Digest {
  if (typeof value !== "string" || !digestPattern.test(value)) {
    inputError(path, "Digests must use sha256:<64-lowercase-hex>.");
  }
}

export function assertCorrelationId(
  value: unknown,
  path = "/correlationId",
): asserts value is string | undefined {
  if (
    value !== undefined
    && (
      typeof value !== "string"
      || value.length === 0
      || utf8Length(value) > 256
      || /[\r\n]/u.test(value)
    )
  ) {
    inputError(
      path,
      "Correlation IDs must contain 1-256 UTF-8 bytes without line breaks.",
    );
  }
}

export function isUriReference(value: unknown): value is string {
  if (
    typeof value !== "string"
    || !/^[\x21-\x7e]*$/u.test(value)
    || /%(?![0-9a-f]{2})/iu.test(value)
  ) {
    return false;
  }
  try {
    new URL(value, "https://flaggo.invalid/");
    return true;
  } catch {
    return false;
  }
}
