import { signalSchemaDigest } from "./canonical.js";
import type {
  AppEmittedMetricSignalDeclaration,
  DerivedMetricSignalDeclaration,
  EventSignalDeclaration,
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

type PrimitiveSignalType = "boolean" | "number" | "string";

export type SignalValue<TType extends PrimitiveSignalType> =
  TType extends "boolean"
    ? boolean
    : TType extends "number"
      ? number
      : string;

export type EventValue<
  TFields extends Record<string, PrimitiveSignalType>,
> = {
  [TKey in keyof TFields]: SignalValue<TFields[TKey]>;
};

type EmittableSignalDeclaration =
  | EventSignalDeclaration
  | AppEmittedMetricSignalDeclaration;

type ValueForDeclaration<
  TDeclaration extends EmittableSignalDeclaration,
> = TDeclaration extends EventSignalDeclaration
  ? EventValue<TDeclaration["fields"]>
  : TDeclaration extends AppEmittedMetricSignalDeclaration
    ? SignalValue<TDeclaration["type"]>
    : never;

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

export function createSignalHandle<
  const TDeclaration extends EmittableSignalDeclaration,
>(
  declaration: TDeclaration,
  sink: TelemetrySink,
): SignalHandle<ValueForDeclaration<TDeclaration>> {
  return createTypedSignalHandle<ValueForDeclaration<TDeclaration>>(
    declaration,
    sink,
  );
}

function createTypedSignalHandle<T>(
  declaration: EmittableSignalDeclaration,
  sink: TelemetrySink,
): SignalHandle<T> {
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

export function createInferenceSignalHandle<
  const TDeclaration extends AppEmittedMetricSignalDeclaration,
>(
  declaration: TDeclaration,
  sink: TelemetrySink,
): InferenceSignalHandle<SignalValue<TDeclaration["type"]>> {
  const handle = createTypedSignalHandle<SignalValue<TDeclaration["type"]>>(
    declaration,
    sink,
  );
  return {
    ...handle,
    input(value) {
      return { signal: { key: declaration.key }, value };
    },
  };
}

export function createDerivedMetricHandle<
  const TDeclaration extends DerivedMetricSignalDeclaration,
>(
  declaration: TDeclaration,
): DerivedMetricHandle<SignalValue<TDeclaration["type"]>> {
  return {
    key: declaration.key,
    schemaDigest: verifiedSchemaDigest(declaration),
    valueType: declaration.type as DerivedMetricHandle<
      SignalValue<TDeclaration["type"]>
    >["valueType"],
  };
}
