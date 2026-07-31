import { signalSchemaDigest } from "./canonical.js";
import type {
  DerivedMetricSignalDeclaration,
  RuntimeContextValue,
  Sha256Digest,
  SignalDeclaration,
  SignalInput,
} from "./types.js";

export interface TelemetryEvent {
  signal: { key: string; schemaDigest: Sha256Digest };
  value: unknown;
  timestamp: string;
}

export interface TelemetrySink {
  emit(event: TelemetryEvent): void;
}

export interface SignalIdentity {
  readonly key: string;
  readonly schemaDigest: Sha256Digest;
}

export interface SignalHandle<T> extends SignalIdentity {
  emit(value: T): void;
}

export interface DerivedMetricHandle<T extends RuntimeContextValue>
  extends SignalIdentity {
  readonly valueType: T extends boolean
    ? "boolean"
    : T extends number
      ? "number"
      : "string";
}

export interface InferenceSignalHandle<T extends RuntimeContextValue>
  extends SignalHandle<T> {
  input(value: T): SignalInput;
}

export interface OpenTelemetryLoggerLike {
  emit(record: {
    body: string;
    attributes: Record<string, boolean | number | string>;
    timestamp: Date;
  }): void;
}

export function createOpenTelemetrySink(
  logger: OpenTelemetryLoggerLike,
): TelemetrySink {
  return {
    emit(event) {
      logger.emit({
        body: "flaggo.signal",
        attributes: {
          "flaggo.signal.key": event.signal.key,
          "flaggo.signal.schema_digest": event.signal.schemaDigest,
          "flaggo.signal.value": serializeAttribute(event.value),
        },
        timestamp: new Date(event.timestamp),
      });
    },
  };
}

function serializeAttribute(value: unknown): boolean | number | string {
  if (
    typeof value === "boolean"
    || typeof value === "number"
    || typeof value === "string"
  ) {
    return value;
  }
  return JSON.stringify(value);
}

function verifiedSchemaDigest(declaration: SignalDeclaration): Sha256Digest {
  const computed = signalSchemaDigest(declaration);
  if (
    declaration.schemaDigest !== undefined
    && declaration.schemaDigest !== computed
  ) {
    throw new Error(`schema digest mismatch for signal key: ${declaration.key}`);
  }
  return computed;
}

export function createSignalHandle<T>(
  declaration: SignalDeclaration,
  sink: TelemetrySink,
): SignalHandle<T> {
  if (
    declaration.kind === "metric"
    && declaration.source === "derived"
  ) {
    throw new Error(
      `derived metric '${declaration.key}' cannot emit application telemetry`,
    );
  }
  const schemaDigest = verifiedSchemaDigest(declaration);
  return {
    key: declaration.key,
    schemaDigest,
    emit(value) {
      sink.emit({
        signal: { key: declaration.key, schemaDigest },
        value,
        timestamp: new Date().toISOString(),
      });
    },
  };
}

export function createInferenceSignalHandle<T extends RuntimeContextValue>(
  declaration: SignalDeclaration,
  sink: TelemetrySink,
): InferenceSignalHandle<T> {
  const handle = createSignalHandle<T>(declaration, sink);
  return {
    ...handle,
    input(value) {
      return { signal: { key: declaration.key }, value };
    },
  };
}

export function createDerivedMetricHandle<T extends RuntimeContextValue>(
  declaration: DerivedMetricSignalDeclaration,
): DerivedMetricHandle<T> {
  return {
    key: declaration.key,
    schemaDigest: verifiedSchemaDigest(declaration),
    valueType: declaration.type as DerivedMetricHandle<T>["valueType"],
  };
}
