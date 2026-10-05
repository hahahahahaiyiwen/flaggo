import {
  FlaggoAbortError,
  FlaggoError,
  FlaggoHttpError,
  FlaggoTimeoutError,
  FlaggoTransportError,
  InvalidServerResponseError,
} from "../errors.js";
import type {
  FlaggoResponse,
  FlaggoResponseMetadata,
  ProblemDetails,
  RequestOptions,
  RetryPolicy,
  TransportConfiguration,
} from "../shared/types.js";
import {
  validateProblemDetails,
} from "../generated/problem-validator.generated.mjs";
import {
  assertCorrelationId,
  hasOnlyKeys,
  inputError,
  isUriReference,
  record,
} from "./guards.js";
import {
  maximumDocumentBytes,
  serializeJson,
  utf8Length,
} from "./serialization.js";
import { normalizeServiceBaseUrl } from "./service-url.js";
import {
  assertResponseSchema,
} from "./validators.js";

const retryableStatuses = new Set([429, 502, 503, 504]);
const defaultRetryPolicy: Required<RetryPolicy> = {
  maxAttempts: 1,
  baseDelayMs: 50,
  maxDelayMs: 1_000,
};

export interface TransportRequest<T> {
  readonly method: "GET" | "POST" | "PUT";
  readonly path: string;
  readonly body?: unknown;
  readonly options?: RequestOptions;
  readonly successStatuses: readonly number[];
  readonly responseDescription: string;
  validateBody?(value: unknown): void;
  parse(value: unknown): T;
}

export interface Transport {
  request<T>(request: TransportRequest<T>): Promise<FlaggoResponse<T>>;
}

function assertRetryPolicy(
  value: RetryPolicy | undefined,
  path: string,
  fallback: Required<RetryPolicy> = defaultRetryPolicy,
): Required<RetryPolicy> {
  const policy = value ?? {};
  const raw = record(policy);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["maxAttempts", "baseDelayMs", "maxDelayMs"]))
  ) {
    return inputError(path, "Retry policy contains unknown members.");
  }
  const maxAttempts = policy.maxAttempts ?? fallback.maxAttempts;
  const baseDelayMs = policy.baseDelayMs ?? fallback.baseDelayMs;
  const maxDelayMs = policy.maxDelayMs ?? fallback.maxDelayMs;
  if (!Number.isInteger(maxAttempts) || maxAttempts < 1 || maxAttempts > 3) {
    return inputError(
      `${path}/maxAttempts`,
      "Retry maxAttempts must be an integer from 1 to 3.",
    );
  }
  if (
    !Number.isInteger(baseDelayMs)
    || baseDelayMs < 0
    || baseDelayMs > 60_000
  ) {
    return inputError(
      `${path}/baseDelayMs`,
      "Retry baseDelayMs must be an integer from 0 to 60000.",
    );
  }
  if (
    !Number.isInteger(maxDelayMs)
    || maxDelayMs < baseDelayMs
    || maxDelayMs > 60_000
  ) {
    return inputError(
      `${path}/maxDelayMs`,
      "Retry maxDelayMs must be an integer from baseDelayMs to 60000.",
    );
  }
  return { maxAttempts, baseDelayMs, maxDelayMs };
}

function assertTimeout(
  value: unknown,
  path: string,
): asserts value is number | undefined {
  if (
    value !== undefined
    && (
      typeof value !== "number"
      || !Number.isInteger(value)
      || value < 1
      || value > 300_000
    )
  ) {
    inputError(path, "Timeouts must be integer milliseconds from 1 to 300000.");
  }
}

function isAbortSignal(value: unknown): value is AbortSignal {
  const candidate = value as Partial<AbortSignal> | undefined;
  return candidate !== undefined
    && typeof candidate.aborted === "boolean"
    && typeof candidate.addEventListener === "function"
    && typeof candidate.removeEventListener === "function";
}

function validateRequestOptions(options: RequestOptions): void {
  const raw = record(options);
  if (
    raw === undefined
    || !hasOnlyKeys(
      raw,
      new Set(["correlationId", "signal", "timeoutMs", "retry"]),
    )
  ) {
    inputError("/options", "Request options contain unknown members.");
  }
  assertCorrelationId(options.correlationId, "/options/correlationId");
  assertTimeout(options.timeoutMs, "/options/timeoutMs");
  if (options.signal !== undefined && !isAbortSignal(options.signal)) {
    inputError("/options/signal", "Signal must implement AbortSignal.");
  }
  if (options.retry !== undefined) {
    assertRetryPolicy(options.retry, "/options/retry");
  }
}

function metadata(response: Response): FlaggoResponseMetadata {
  const correlationId = response.headers.get("X-Flaggo-Correlation-Id");
  if (
    correlationId !== null
    && (
      correlationId.length === 0
      || utf8Length(correlationId) > 256
      || /[\r\n]/u.test(correlationId)
    )
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned an invalid correlation ID header.",
    );
  }

  const retryAfter = response.headers.get("Retry-After");
  let retryAfterSeconds: number | undefined;
  if (retryAfter !== null) {
    retryAfterSeconds = Number(retryAfter);
    if (!Number.isFinite(retryAfterSeconds) || retryAfterSeconds < 0) {
      throw new InvalidServerResponseError(
        "Flaggo returned an invalid Retry-After header.",
      );
    }
  }

  const location = response.headers.get("Location");
  if (
    location !== null
    && (
      location.length === 0
      || utf8Length(location) > 2_048
      || /[\r\n]/u.test(location)
      || !isUriReference(location)
    )
  ) {
    throw new InvalidServerResponseError(
      "Flaggo returned an invalid Location header.",
    );
  }

  return {
    status: response.status,
    ...(correlationId === null ? {} : { correlationId }),
    ...(retryAfterSeconds === undefined ? {} : { retryAfterSeconds }),
    ...(location === null ? {} : { location }),
  };
}

function contentType(response: Response): string {
  return response.headers.get("Content-Type")
    ?.split(";", 1)[0]
    ?.trim()
    .toLowerCase() ?? "";
}

async function parseJson(
  response: Response,
  signal: AbortSignal,
): Promise<unknown> {
  const expected = response.ok
    ? "application/json"
    : "application/problem+json";
  const actual = contentType(response);
  if (actual !== expected) {
    throw new InvalidServerResponseError(
      `Flaggo returned Content-Type '${actual}' for HTTP ${response.status}; expected '${expected}'.`,
    );
  }

  const body = await responseText(response, signal);
  try {
    return JSON.parse(body) as unknown;
  } catch (error) {
    throw new InvalidServerResponseError(
      `Flaggo returned non-JSON HTTP ${response.status}.`,
      [],
      { cause: error },
    );
  }
}

async function responseText(
  response: Response,
  signal: AbortSignal,
): Promise<string> {
  const contentLength = response.headers.get("Content-Length");
  if (contentLength !== null) {
    const bytes = Number(contentLength);
    if (!Number.isInteger(bytes) || bytes < 0) {
      throw new InvalidServerResponseError(
        "Flaggo returned an invalid Content-Length header.",
      );
    }
    if (bytes > maximumDocumentBytes) {
      throw new InvalidServerResponseError(
        `Flaggo returned a JSON document larger than ${maximumDocumentBytes} UTF-8 bytes.`,
      );
    }
  }

  try {
    if (response.body === null) {
      const body = await abortable(response.text(), signal);
      if (utf8Length(body) > maximumDocumentBytes) {
        throw new InvalidServerResponseError(
          `Flaggo returned a JSON document larger than ${maximumDocumentBytes} UTF-8 bytes.`,
        );
      }
      return body;
    }

    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let totalBytes = 0;
    try {
      while (true) {
        const next = await abortable(reader.read(), signal);
        if (next.done) break;
        totalBytes += next.value.byteLength;
        if (totalBytes > maximumDocumentBytes) {
          await abortable(reader.cancel(), signal);
          throw new InvalidServerResponseError(
            `Flaggo returned a JSON document larger than ${maximumDocumentBytes} UTF-8 bytes.`,
          );
        }
        chunks.push(next.value);
      }
    } finally {
      reader.releaseLock();
    }

    const bytes = new Uint8Array(totalBytes);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch (error) {
      throw new InvalidServerResponseError(
        `Flaggo returned invalid UTF-8 for HTTP ${response.status}.`,
        [],
        { cause: error },
      );
    }
  } catch (error) {
    if (signal.aborted || error instanceof FlaggoError) throw error;
    throw new FlaggoTransportError(
      `The Flaggo response body could not be read for HTTP ${response.status}.`,
      { cause: error },
    );
  }
}

async function discardResponseBody(
  response: Response,
  signal: AbortSignal,
): Promise<void> {
  if (response.body === null) return;
  try {
    await abortable(response.body.cancel(), signal);
  } catch (error) {
    if (signal.aborted || error instanceof FlaggoError) throw error;
    throw new FlaggoTransportError(
      `The Flaggo response body could not be discarded for HTTP ${response.status}.`,
      { cause: error },
    );
  }
}

function problemOrThrow(value: unknown, response: Response): ProblemDetails {
  assertResponseSchema(validateProblemDetails, value, "Problem Details");
  const problem = value as ProblemDetails;
  if (problem.status !== undefined && problem.status !== response.status) {
    throw new InvalidServerResponseError(
      `Flaggo returned malformed Problem Details for HTTP ${response.status}.`,
    );
  }
  return problem;
}

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(signal.reason);
  return new Promise<T>((resolve, reject) => {
    const onAbort = (): void => reject(signal.reason);
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener("abort", onAbort);
        reject(error);
      },
    );
  });
}

function sleep(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (milliseconds === 0) return Promise.resolve();
  if (signal.aborted) return Promise.reject(signal.reason);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, milliseconds);
    const onAbort = (): void => {
      clearTimeout(timer);
      signal.removeEventListener("abort", onAbort);
      reject(signal.reason);
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

function requestHeaders(
  correlationId: string | undefined,
  hasBody: boolean,
): Headers {
  const headers = new Headers({
    Accept: "application/json, application/problem+json",
  });
  if (hasBody) headers.set("Content-Type", "application/json");
  if (correlationId !== undefined) {
    headers.set("X-Flaggo-Correlation-Id", correlationId);
  }
  return headers;
}

function retryDelay(
  response: Response | undefined,
  failedAttempt: number,
  policy: Required<RetryPolicy>,
): number {
  const retryAfter = response?.headers.get("Retry-After");
  if (retryAfter !== null && retryAfter !== undefined) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds) && seconds >= 0) {
      return Math.min(policy.maxDelayMs, Math.round(seconds * 1_000));
    }
  }
  return Math.min(
    policy.maxDelayMs,
    policy.baseDelayMs * (2 ** (failedAttempt - 1)),
  );
}

function abortError(
  timedOut: boolean,
  timeoutMs: number | undefined,
  cause: unknown,
): FlaggoAbortError | FlaggoTimeoutError {
  return timedOut
    ? new FlaggoTimeoutError(timeoutMs!, { cause })
    : new FlaggoAbortError({ cause });
}

export function createTransport(
  configuration: TransportConfiguration,
): Transport {
  assertTimeout(configuration.timeoutMs, "/timeoutMs");
  const defaultPolicy = assertRetryPolicy(configuration.retry, "/retry");
  const baseUrl = normalizeServiceBaseUrl(configuration.baseUrl, "/baseUrl");
  const fetchImplementation = configuration.fetch
    ?? globalThis.fetch?.bind(globalThis);
  if (typeof fetchImplementation !== "function") {
    inputError(
      "/fetch",
      "A Fetch-compatible implementation is required in this runtime.",
    );
  }

  return {
    async request<T>(
      request: TransportRequest<T>,
    ): Promise<FlaggoResponse<T>> {
      const options = request.options ?? {};
      validateRequestOptions(options);
      const policy = options.retry === undefined
        ? defaultPolicy
        : assertRetryPolicy(options.retry, "/options/retry", defaultPolicy);
      const timeoutMs = options.timeoutMs ?? configuration.timeoutMs;
      const serialized = request.body === undefined
        ? undefined
        : serializeJson(request.body);
      if (serialized !== undefined) {
        request.validateBody?.(serialized.value);
      }
      const controller = new AbortController();
      let timedOut = false;
      let timeout: ReturnType<typeof setTimeout> | undefined;
      const onCallerAbort = (): void => controller.abort(options.signal?.reason);

      if (options.signal?.aborted === true) {
        controller.abort(options.signal.reason);
      } else {
        options.signal?.addEventListener("abort", onCallerAbort, { once: true });
      }
      if (timeoutMs !== undefined) {
        timeout = setTimeout(() => {
          timedOut = true;
          controller.abort(new Error("Flaggo request timeout."));
        }, timeoutMs);
      }

      try {
        for (let attempt = 1; attempt <= policy.maxAttempts; attempt += 1) {
          if (controller.signal.aborted) {
            throw abortError(timedOut, timeoutMs, controller.signal.reason);
          }

          let response: Response;
          try {
            response = await abortable(fetchImplementation(
              `${baseUrl}${request.path}`,
              {
                method: request.method,
                headers: requestHeaders(
                  options.correlationId,
                  serialized !== undefined,
                ),
                ...(serialized === undefined ? {} : { body: serialized.body }),
                signal: controller.signal,
              },
            ), controller.signal);
          } catch (error) {
            if (controller.signal.aborted) {
              throw abortError(timedOut, timeoutMs, error);
            }
            if (error instanceof FlaggoError) throw error;
            if (attempt < policy.maxAttempts) {
              await sleep(
                retryDelay(undefined, attempt, policy),
                controller.signal,
              );
              continue;
            }
            throw new FlaggoTransportError(
              "The Flaggo request could not reach the service.",
              { cause: error },
            );
          }

          if (
            retryableStatuses.has(response.status)
            && attempt < policy.maxAttempts
          ) {
            await discardResponseBody(response, controller.signal);
            await sleep(
              retryDelay(response, attempt, policy),
              controller.signal,
            );
            continue;
          }

          const responseBody = await parseJson(response, controller.signal);
          const responseMetadata = metadata(response);
          if (!response.ok) {
            throw new FlaggoHttpError(
              problemOrThrow(responseBody, response),
              responseMetadata,
            );
          }
          if (!request.successStatuses.includes(response.status)) {
            throw new InvalidServerResponseError(
              `Flaggo returned unexpected HTTP ${response.status} for ${request.responseDescription}.`,
            );
          }
          return {
            value: request.parse(responseBody),
            metadata: responseMetadata,
          };
        }
        throw new FlaggoTransportError(
          "Flaggo retry processing ended unexpectedly.",
        );
      } catch (error) {
        if (
          controller.signal.aborted
          && !(error instanceof FlaggoAbortError)
          && !(error instanceof FlaggoTimeoutError)
        ) {
          throw abortError(timedOut, timeoutMs, error);
        }
        throw error;
      } finally {
        if (timeout !== undefined) clearTimeout(timeout);
        options.signal?.removeEventListener("abort", onCallerAbort);
      }
    },
  };
}
