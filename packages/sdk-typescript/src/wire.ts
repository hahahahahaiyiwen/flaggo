import { InvalidServerResponseError } from "./errors.js";
import type {
  FlaggoResponseMetadata,
  ProblemDetails,
  Sha256Digest,
} from "./types.js";

const standardProblemKeys = new Set([
  "type",
  "title",
  "status",
  "detail",
  "instance",
]);

export type CredentialProvider =
  | { mode: "local-development" }
  | { mode: "bearer"; getToken(): Promise<string> };

export async function authorization(
  credential: CredentialProvider | undefined,
): Promise<string | undefined> {
  if (credential === undefined) return undefined;
  if (credential.mode === "local-development") return "Flaggo-Local-Development";
  return `Bearer ${await credential.getToken()}`;
}

export function headers(
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

export async function parseJson(response: Response): Promise<unknown> {
  const contentType = response.headers.get("Content-Type")?.split(";", 1)[0]?.trim()
    .toLowerCase();
  if (contentType !== "application/json" && contentType !== "application/problem+json") {
    throw new InvalidServerResponseError(
      `Flaggo returned unsupported Content-Type '${contentType ?? ""}' for HTTP ${response.status}.`,
    );
  }
  try {
    return await response.json();
  } catch (error) {
    throw new InvalidServerResponseError(
      `Flaggo returned non-JSON HTTP ${response.status}.`,
      { cause: error },
    );
  }
}

export function problemOrThrow(value: unknown, status: number): ProblemDetails {
  const problem = record(value);
  if (
    problem === undefined
    || !hasOnlyKeys(problem, standardProblemKeys)
    || !isUriReference(problem.type)
    || problem.status !== status
    || (problem.title !== undefined && typeof problem.title !== "string")
    || (problem.detail !== undefined && typeof problem.detail !== "string")
    || (problem.instance !== undefined && !isUriReference(problem.instance))
  ) {
    throw new InvalidServerResponseError(
      `Flaggo returned malformed Problem Details for HTTP ${status}.`,
    );
  }
  return problem as unknown as ProblemDetails;
}

export function responseMetadata(response: Response): FlaggoResponseMetadata {
  const correlationId = response.headers.get("X-Flaggo-Correlation-Id");
  const retryAfter = response.headers.get("Retry-After");
  const seconds = retryAfter === null ? undefined : Number(retryAfter);
  return {
    ...(correlationId === null ? {} : { correlationId }),
    ...(seconds === undefined || !Number.isFinite(seconds) || seconds < 0
      ? {}
      : { retryAfterSeconds: seconds }),
  };
}

export function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

export function hasOnlyKeys(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string>,
): boolean {
  return Object.keys(value).every((key) => allowed.has(key));
}

export function isSha256Digest(value: unknown): value is Sha256Digest {
  return typeof value === "string" && /^sha256:[0-9a-f]{64}$/u.test(value);
}

export function isDecisionName(value: unknown): value is string {
  return typeof value === "string"
    && /^[A-Za-z][A-Za-z0-9._-]{0,127}$/u.test(value);
}

function isUriReference(value: unknown): value is string {
  return typeof value === "string"
    && /^[\x21-\x7e]*$/u.test(value)
    && !/%(?![0-9a-f]{2})/iu.test(value)
    && URL.canParse(value, "https://flaggo.invalid/");
}
