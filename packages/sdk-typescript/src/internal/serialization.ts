import { InvalidFlaggoInputError } from "../errors.js";
import type { JsonValue, SdkValidationIssue } from "../shared/types.js";

export const maximumDocumentBytes = 262_144;
export const maximumJsonDepth = 16;
export const maximumCollectionSize = 256;
export const maximumStringBytes = 16_384;

const encoder = new TextEncoder();

export function utf8Length(value: string): number {
  return encoder.encode(value).byteLength;
}

function inputError(path: string, message: string): never {
  const issue: SdkValidationIssue = { path, message };
  throw new InvalidFlaggoInputError(`${path || "/"}: ${message}`, [issue]);
}

function propertyPath(path: string, property: string): string {
  const escaped = property.replace(/~/gu, "~0").replace(/\//gu, "~1");
  return `${path}/${escaped}`;
}

function cloneJson(
  value: unknown,
  path: string,
  depth: number,
): JsonValue {
  if (depth > maximumJsonDepth) {
    return inputError(
      path,
      `Values may be nested at most ${maximumJsonDepth} levels.`,
    );
  }
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      return inputError(path, "Numbers must be finite.");
    }
    return value;
  }
  if (typeof value === "string") {
    if (utf8Length(value) > maximumStringBytes) {
      return inputError(
        path,
        `Strings may contain at most ${maximumStringBytes} UTF-8 bytes.`,
      );
    }
    return value;
  }
  if (Array.isArray(value)) {
    if (value.length > maximumCollectionSize) {
      return inputError(
        path,
        `Arrays may contain at most ${maximumCollectionSize} items.`,
      );
    }
    const keys = Reflect.ownKeys(value);
    if (
      keys.some((key) =>
        typeof key !== "string"
        || (
          key !== "length"
          && (!/^(0|[1-9][0-9]*)$/u.test(key) || Number(key) >= value.length)
        ))
    ) {
      return inputError(path, "Arrays must not contain custom properties.");
    }
    const clone: JsonValue[] = [];
    for (let index = 0; index < value.length; index += 1) {
      if (!Object.hasOwn(value, index)) {
        return inputError(`${path}/${index}`, "Sparse arrays are not JSON values.");
      }
      const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
      if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
        return inputError(`${path}/${index}`, "Array items must be enumerable data values.");
      }
      clone.push(cloneJson(descriptor.value, `${path}/${index}`, depth + 1));
    }
    return clone;
  }
  if (typeof value !== "object") {
    return inputError(path, "Values must be JSON values.");
  }

  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    return inputError(path, "Objects must be plain JSON objects.");
  }
  const keys = Reflect.ownKeys(value);
  if (keys.length > maximumCollectionSize) {
    return inputError(
      path,
      `Objects may contain at most ${maximumCollectionSize} properties.`,
    );
  }

  const clone: Record<string, JsonValue> = Object.create(null);
  for (const key of keys) {
    if (typeof key !== "string") {
      return inputError(path, "JSON objects cannot contain symbol properties.");
    }
    if (utf8Length(key) > maximumStringBytes) {
      return inputError(
        path,
        `Object property names may contain at most ${maximumStringBytes} UTF-8 bytes.`,
      );
    }
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return inputError(
        propertyPath(path, key),
        "Object properties must be enumerable data values.",
      );
    }
    clone[key] = cloneJson(
      descriptor.value,
      propertyPath(path, key),
      depth + 1,
    );
  }
  return clone;
}

export function cloneJsonValue(value: unknown, path = ""): JsonValue {
  return cloneJson(value, path, 0);
}

export function serializeJson(value: unknown): {
  readonly body: string;
  readonly value: JsonValue;
} {
  const clone = cloneJsonValue(value);
  const body = JSON.stringify(clone);
  if (utf8Length(body) > maximumDocumentBytes) {
    return inputError(
      "",
      `JSON documents may contain at most ${maximumDocumentBytes} UTF-8 bytes.`,
    );
  }
  return { body, value: clone };
}
