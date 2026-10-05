import type { JsonValue as GeneratedJsonValue } from "../generated/runtime-models.generated.js";
import type { RFC9457ProblemDetails } from "../generated/problem-details.generated.js";

export type DeepReadonly<T> =
  T extends readonly (infer TValue)[]
    ? readonly DeepReadonly<TValue>[]
    : T extends object
      ? { readonly [TKey in keyof T]: DeepReadonly<T[TKey]> }
      : T;

export type JsonValue = DeepReadonly<GeneratedJsonValue>;
export type JsonObject = Readonly<Record<string, JsonValue>>;
export type Sha256Digest = `sha256:${string}`;

export type ProblemDetails = DeepReadonly<RFC9457ProblemDetails>;

export interface FlaggoResponseMetadata {
  readonly status: number;
  readonly correlationId?: string;
  readonly retryAfterSeconds?: number;
  readonly location?: string;
}

export interface FlaggoResponse<T> {
  readonly value: T;
  readonly metadata: FlaggoResponseMetadata;
}

export type FetchLike = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export interface RetryPolicy {
  readonly maxAttempts?: number;
  readonly baseDelayMs?: number;
  readonly maxDelayMs?: number;
}

export interface RequestOptions {
  readonly correlationId?: string;
  readonly signal?: AbortSignal;
  readonly timeoutMs?: number;
  readonly retry?: RetryPolicy;
}

export interface TransportConfiguration {
  readonly baseUrl: string | URL;
  readonly fetch?: FetchLike;
  readonly timeoutMs?: number;
  readonly retry?: RetryPolicy;
}

export interface SdkValidationIssue {
  readonly path: string;
  readonly message: string;
  readonly keyword?: string;
}
