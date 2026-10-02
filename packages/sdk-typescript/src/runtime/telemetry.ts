import type { Logger, LogRecord } from "@opentelemetry/api-logs";
import { logs } from "@opentelemetry/api-logs";

import { SDK_VERSION } from "../generated/package-version.generated.js";
import {
  hasOnlyKeys,
  inputError,
  record,
} from "../internal/guards.js";
import { serializeJson } from "../internal/serialization.js";
import type {
  FlaggoResponseMetadata,
  JsonValue,
  Sha256Digest,
} from "../shared/types.js";
import type { RuntimeDecision, RuntimeEvaluation } from "./types.js";

export type FlaggoTelemetryLogger = Pick<Logger, "emit">;

export interface CorrelationAttributes {
  readonly [name: string]: string | number | boolean;
}

export interface FlaggoTelemetryConfiguration {
  readonly logger?: FlaggoTelemetryLogger;
}

export interface DecisionTelemetryContext {
  readonly decisionId: string;
  readonly contractName: string;
  readonly contractDigest: Sha256Digest;
  readonly executableDigest: Sha256Digest;
  readonly result: JsonValue;
  readonly resultHash: Sha256Digest;
  readonly evaluation: RuntimeEvaluation;
  readonly correlation?: CorrelationAttributes;
}

export interface DecisionReceivedTelemetryInput<TResult extends JsonValue = JsonValue> {
  readonly contractName: string;
  readonly decision: RuntimeDecision<TResult>;
  readonly metadata?: FlaggoResponseMetadata;
  readonly correlation?: CorrelationAttributes;
}

export interface FlaggoTelemetry {
  readonly logger: FlaggoTelemetryLogger;
  recordDecisionReceived<TResult extends JsonValue>(
    input: DecisionReceivedTelemetryInput<TResult>,
  ): DecisionTelemetryContext;
}

const defaultLogger = logs.getLogger("@flaggo/sdk", SDK_VERSION);
const correlationNamePattern = /^[A-Za-z][A-Za-z0-9_]{0,127}$/u;
type TelemetryAttributes = NonNullable<LogRecord["attributes"]>;

export function createFlaggoTelemetry(
  configuration: FlaggoTelemetryConfiguration = {},
): FlaggoTelemetry {
  const logger = validateTelemetryConfiguration(configuration);
  return {
    logger,
    recordDecisionReceived(input) {
      return recordDecisionReceived(logger, input);
    },
  };
}

export function recordDecisionReceived<TResult extends JsonValue>(
  logger: FlaggoTelemetryLogger,
  input: DecisionReceivedTelemetryInput<TResult>,
): DecisionTelemetryContext {
  const decisionId = createDecisionId();
  const resultJson = serializeJson(input.decision.result).body;
  const resultHash = sha256(resultJson);
  const correlation = validateCorrelation(input.correlation, "/correlation");
  const context: DecisionTelemetryContext = {
    decisionId,
    contractName: input.contractName,
    contractDigest: input.decision.contractDigest,
    executableDigest: input.decision.executableDigest,
    result: input.decision.result,
    resultHash,
    evaluation: input.decision.evaluation,
    ...(correlation === undefined ? {} : { correlation }),
  };
  logger.emit({
    eventName: "flaggo.decision.received",
    attributes: {
      "flaggo.signal": "decision.received",
      "flaggo.decision.id": decisionId,
      "flaggo.contract.name": input.contractName,
      "flaggo.contract.digest": input.decision.contractDigest,
      "flaggo.executable.digest": input.decision.executableDigest,
      "flaggo.result.json": resultJson,
      "flaggo.result.hash": resultHash,
      ...evaluationAttributes(input.decision.evaluation),
      ...(input.metadata?.correlationId === undefined
        ? {}
        : { "flaggo.request.correlation_id": input.metadata.correlationId }),
      ...correlationAttributes(input.correlation),
    },
  });
  return context;
}

export function emitDecisionReceived<TResult extends JsonValue>(
  telemetry: FlaggoTelemetry,
  input: DecisionReceivedTelemetryInput<TResult>,
): DecisionTelemetryContext {
  return telemetry.recordDecisionReceived(input);
}

function validateTelemetryConfiguration(
  configuration: FlaggoTelemetryConfiguration,
): FlaggoTelemetryLogger {
  const raw = record(configuration);
  if (
    raw === undefined
    || !hasOnlyKeys(raw, new Set(["logger"]))
  ) {
    inputError(
      "/telemetry",
      "Telemetry configuration contains unknown members.",
    );
  }
  if (
    configuration.logger !== undefined
    && typeof configuration.logger.emit !== "function"
  ) {
    inputError("/telemetry/logger", "Telemetry logger must implement emit().");
  }
  return configuration.logger ?? defaultLogger;
}

function evaluationAttributes(
  evaluation: RuntimeEvaluation,
): TelemetryAttributes {
  if (evaluation.source === "rule") {
    return {
      "flaggo.evaluation.source": "rule",
      "flaggo.evaluation.rule": evaluation.rule,
    };
  }
  return { "flaggo.evaluation.source": "default" };
}

function correlationAttributes(
  correlation: CorrelationAttributes | undefined,
): TelemetryAttributes {
  const validated = validateCorrelation(correlation, "/correlation");
  if (validated === undefined) return {};
  return Object.fromEntries(
    Object.entries(validated).map(([name, value]) => [
      `flaggo.correlation.${name}`,
      value,
    ]),
  );
}

function validateCorrelation(
  correlation: CorrelationAttributes | undefined,
  path: string,
): CorrelationAttributes | undefined {
  if (correlation === undefined) return undefined;
  const raw = record(correlation);
  if (raw === undefined) {
    inputError(path, "Correlation attributes must be a plain object.");
  }
  const result: Record<string, string | number | boolean> = Object.create(null);
  for (const [name, value] of Object.entries(raw)) {
    if (!correlationNamePattern.test(name)) {
      inputError(
        `${path}/${name}`,
        "Correlation attribute names must begin with a letter and contain only letters, digits, and underscores.",
      );
    }
    if (
      typeof value !== "string"
      && typeof value !== "number"
      && typeof value !== "boolean"
    ) {
      inputError(
        `${path}/${name}`,
        "Correlation attribute values must be strings, numbers, or booleans.",
      );
    }
    result[name] = value;
  }
  return result;
}

function createDecisionId(): string {
  if (globalThis.crypto?.randomUUID !== undefined) {
    return globalThis.crypto.randomUUID();
  }
  const bytes = new Uint8Array(16);
  globalThis.crypto?.getRandomValues(bytes);
  if (bytes.every((value) => value === 0)) {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return [
    hex.slice(0, 4).join(""),
    hex.slice(4, 6).join(""),
    hex.slice(6, 8).join(""),
    hex.slice(8, 10).join(""),
    hex.slice(10).join(""),
  ].join("-");
}

function sha256(value: string): Sha256Digest {
  const bytes = new TextEncoder().encode(value);
  const hash = sha256Bytes(bytes);
  return `sha256:${Array.from(hash, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function sha256Bytes(message: Uint8Array): Uint8Array {
  const words = new Uint32Array(64);
  const bitLength = message.length * 8;
  const paddedLength = Math.ceil((message.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(message);
  padded[message.length] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(paddedLength - 4, bitLength, false);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;

  for (let offset = 0; offset < padded.length; offset += 64) {
    for (let index = 0; index < 16; index += 1) {
      words[index] = view.getUint32(offset + index * 4, false);
    }
    for (let index = 16; index < 64; index += 1) {
      const s0 = rotateRight(words[index - 15]!, 7)
        ^ rotateRight(words[index - 15]!, 18)
        ^ (words[index - 15]! >>> 3);
      const s1 = rotateRight(words[index - 2]!, 17)
        ^ rotateRight(words[index - 2]!, 19)
        ^ (words[index - 2]! >>> 10);
      words[index] = add32(words[index - 16]!, s0, words[index - 7]!, s1);
    }

    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let h = h7;

    for (let index = 0; index < 64; index += 1) {
      const s1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const ch = (e & f) ^ (~e & g);
      const temp1 = add32(h, s1, ch, k[index]!, words[index]!);
      const s0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = add32(s0, maj);

      h = g;
      g = f;
      f = e;
      e = add32(d, temp1);
      d = c;
      c = b;
      b = a;
      a = add32(temp1, temp2);
    }

    h0 = add32(h0, a);
    h1 = add32(h1, b);
    h2 = add32(h2, c);
    h3 = add32(h3, d);
    h4 = add32(h4, e);
    h5 = add32(h5, f);
    h6 = add32(h6, g);
    h7 = add32(h7, h);
  }

  const output = new Uint8Array(32);
  const outputView = new DataView(output.buffer);
  [h0, h1, h2, h3, h4, h5, h6, h7].forEach((word, index) => {
    outputView.setUint32(index * 4, word, false);
  });
  return output;
}

function rotateRight(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
}

function add32(...values: readonly number[]): number {
  return values.reduce((sum, value) => (sum + value) >>> 0, 0);
}

const k = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
  0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
  0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
  0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
  0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
  0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
] as const;
