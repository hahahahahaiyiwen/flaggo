import type {
  NumberDecisionDefinition,
  PolicyConstraint,
  SignalDeclaration,
} from "./types.js";

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function exactKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = [],
): boolean {
  const allowed = new Set([...required, ...optional]);
  return required.every((key) => Object.hasOwn(value, key))
    && Object.keys(value).every((key) => allowed.has(key));
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function optionalString(value: unknown): boolean {
  return value === undefined || typeof value === "string";
}

function digest(value: unknown): boolean {
  return value === undefined
    || (
      typeof value === "string"
      && /^sha256:[0-9a-f]{64}$/u.test(value)
    );
}

function stringArray(value: unknown, minimum = 0): value is string[] {
  return Array.isArray(value)
    && value.length >= minimum
    && value.every((item) => typeof item === "string");
}

function signalRef(value: unknown): boolean {
  const candidate = record(value);
  return candidate !== undefined
    && exactKeys(candidate, ["key"])
    && nonEmptyString(candidate.key);
}

function signalRefArray(value: unknown, minimum = 0): boolean {
  return Array.isArray(value)
    && value.length >= minimum
    && value.every(signalRef);
}

function numericRange(value: unknown): boolean {
  return Array.isArray(value)
    && value.length === 2
    && value.every(finite)
    && value[0]! <= value[1]!;
}

interface DecimalValue {
  coefficient: bigint;
  scale: number;
}

function decimalValue(value: number): DecimalValue {
  const [mantissa, exponentText] = value.toString().toLowerCase().split("e");
  const exponent = exponentText === undefined ? 0 : Number(exponentText);
  const negative = mantissa!.startsWith("-");
  const unsigned = negative || mantissa!.startsWith("+")
    ? mantissa!.slice(1)
    : mantissa!;
  const [whole, fraction = ""] = unsigned.split(".");
  let coefficient = BigInt(`${whole}${fraction}`);
  if (negative) coefficient = -coefficient;
  let scale = fraction.length - exponent;
  if (scale < 0) {
    coefficient *= 10n ** BigInt(-scale);
    scale = 0;
  }
  return { coefficient, scale };
}

function stepAligned(value: number, minimum: number, step: number): boolean {
  const values = [
    decimalValue(value),
    decimalValue(minimum),
    decimalValue(step),
  ];
  const scale = Math.max(...values.map((item) => item.scale));
  const scaled = values.map((item) =>
    item.coefficient * 10n ** BigInt(scale - item.scale)
  );
  return (scaled[0]! - scaled[1]!) % scaled[2]! === 0n;
}

function eventSignal(value: Record<string, unknown>): boolean {
  const fields = record(value.fields);
  const units = value.units === undefined ? undefined : record(value.units);
  return value.kind === "event"
    && exactKeys(value, ["kind", "key", "fields"], ["units", "schemaDigest"])
    && nonEmptyString(value.key)
    && fields !== undefined
    && Object.values(fields).every((field) =>
      field === "boolean" || field === "number" || field === "string"
    )
    && (
      units === undefined
      || Object.values(units).every((unit) => typeof unit === "string")
    )
    && digest(value.schemaDigest);
}

function appMetricSignal(value: Record<string, unknown>): boolean {
  return value.kind === "metric"
    && value.source === "app-emitted"
    && exactKeys(
      value,
      ["kind", "key", "type", "source"],
      ["unit", "range", "schemaDigest"],
    )
    && nonEmptyString(value.key)
    && (
      value.type === "boolean"
      || value.type === "number"
      || value.type === "string"
    )
    && optionalString(value.unit)
    && (value.range === undefined || numericRange(value.range))
    && digest(value.schemaDigest);
}

function derivedMetricSignal(value: Record<string, unknown>): boolean {
  return value.kind === "metric"
    && value.source === "derived"
    && exactKeys(
      value,
      ["kind", "key", "type", "source", "from", "aggregation", "window"],
      ["unit", "range", "schemaDigest"],
    )
    && nonEmptyString(value.key)
    && (
      value.type === "boolean"
      || value.type === "number"
      || value.type === "string"
    )
    && signalRefArray(value.from, 1)
    && typeof value.aggregation === "string"
    && typeof value.window === "string"
    && optionalString(value.unit)
    && (value.range === undefined || numericRange(value.range))
    && digest(value.schemaDigest);
}

export function isFrozenSignalDeclaration(
  value: unknown,
): value is SignalDeclaration {
  const candidate = record(value);
  return candidate !== undefined
    && (
      eventSignal(candidate)
      || appMetricSignal(candidate)
      || derivedMetricSignal(candidate)
    );
}

function runtimeContextSchema(value: unknown): boolean {
  const schema = record(value);
  return schema !== undefined
    && Object.values(schema).every((entry) => {
      const candidate = record(entry);
      return candidate !== undefined
        && exactKeys(candidate, ["type"], ["required", "target"])
        && (
          candidate.type === "boolean"
          || candidate.type === "number"
          || candidate.type === "string"
        )
        && (
          candidate.required === undefined
          || typeof candidate.required === "boolean"
        )
        && optionalString(candidate.target);
    });
}

function signalReferences(value: unknown): boolean {
  const references = record(value);
  return references !== undefined
    && exactKeys(references, [], ["allowed", "evidence", "guardrails"])
    && ["allowed", "evidence", "guardrails"].every((key) =>
      references[key] === undefined || signalRefArray(references[key])
    );
}

function inference(value: unknown): boolean {
  const candidate = record(value);
  return candidate !== undefined
    && exactKeys(candidate, ["target"], ["inputs", "fallbackOrder"])
    && typeof candidate.target === "string"
    && (candidate.inputs === undefined || signalRefArray(candidate.inputs))
    && (
      candidate.fallbackOrder === undefined
      || stringArray(candidate.fallbackOrder)
    );
}

function metricObjective(value: unknown): boolean {
  const candidate = record(value);
  if (candidate === undefined || !signalRef(candidate.signal)) return false;
  if (candidate.direction === "target") {
    return exactKeys(candidate, ["signal", "direction", "target"])
      && finite(candidate.target);
  }
  return (
    candidate.direction === "minimize"
    || candidate.direction === "maximize"
  ) && exactKeys(candidate, ["signal", "direction"]);
}

function intent(value: unknown): boolean {
  const candidate = record(value);
  if (candidate === undefined) return false;
  if (candidate.type === "natural-language") {
    return exactKeys(candidate, ["type", "text"])
      && typeof candidate.text === "string";
  }
  return candidate.type === "metric-objective"
    && exactKeys(candidate, ["type", "primary"], ["secondary", "rationale"])
    && metricObjective(candidate.primary)
    && (
      candidate.secondary === undefined
      || (
        Array.isArray(candidate.secondary)
        && candidate.secondary.every(metricObjective)
      )
    )
    && optionalString(candidate.rationale);
}

function onlineStrategy(value: unknown): boolean {
  const candidate = record(value);
  return candidate !== undefined
    && exactKeys(candidate, ["mode"], ["liveInputs"])
    && (
      candidate.mode === "active-value"
      || candidate.mode === "approved-strategy"
      || candidate.mode === "experiment"
      || candidate.mode === "fallback-only"
    )
    && (
      candidate.liveInputs === undefined
      || stringArray(candidate.liveInputs)
    );
}

function policyConstraint(value: unknown): value is PolicyConstraint {
  const candidate = record(value);
  if (candidate === undefined || typeof candidate.kind !== "string") {
    return false;
  }
  switch (candidate.kind) {
    case "number-bounds":
      return exactKeys(candidate, ["kind", "min", "max"])
        && finite(candidate.min)
        && finite(candidate.max)
        && candidate.min <= candidate.max;
    case "max-delta":
      return exactKeys(candidate, ["kind", "value"])
        && finite(candidate.value);
    case "cooldown":
      return exactKeys(candidate, ["kind", "seconds"])
        && finite(candidate.seconds)
        && candidate.seconds >= 0;
    case "min-evidence-quality":
    case "max-model-uncertainty":
    case "min-expected-outcome":
      return exactKeys(candidate, ["kind", "value"])
        && finite(candidate.value)
        && candidate.value >= 0
        && candidate.value <= 1;
    case "min-sample-size":
      return exactKeys(candidate, ["kind", "value"])
        && finite(candidate.value)
        && candidate.value >= 0;
    case "pause":
      return exactKeys(candidate, ["kind", "paused"])
        && typeof candidate.paused === "boolean";
    default:
      return false;
  }
}

function policy(value: unknown): boolean {
  const candidate = record(value);
  if (candidate === undefined) return false;
  if (candidate.kind === "reference") {
    return exactKeys(candidate, ["kind", "policyId"])
      && typeof candidate.policyId === "string";
  }
  if (
    candidate.kind !== "inline"
    || !exactKeys(candidate, ["kind", "constraints"], ["clientFallback"])
    || !Array.isArray(candidate.constraints)
    || !candidate.constraints.every(policyConstraint)
  ) {
    return false;
  }
  if (candidate.clientFallback === undefined) return true;
  const fallback = record(candidate.clientFallback);
  return fallback !== undefined
    && exactKeys(fallback, ["requiredEvidenceUnavailable"])
    && (
      fallback.requiredEvidenceUnavailable === "allow"
      || fallback.requiredEvidenceUnavailable === "forbid"
    );
}

export function isFrozenNumberDecisionDefinition(
  value: unknown,
): value is NumberDecisionDefinition {
  const candidate = record(value);
  if (
    candidate === undefined
    || !exactKeys(
      candidate,
      ["key", "valueType", "actionSpace", "fallback", "policy"],
      [
        "definitionId",
        "owner",
        "runtimeContextSchema",
        "targetHierarchy",
        "signals",
        "inference",
        "intent",
        "onlineStrategy",
      ],
    )
    || !nonEmptyString(candidate.key)
    || !optionalString(candidate.definitionId)
    || !optionalString(candidate.owner)
    || candidate.valueType !== "number"
    || (candidate.intent !== undefined && !intent(candidate.intent))
    || !policy(candidate.policy)
    || (
      candidate.runtimeContextSchema !== undefined
      && !runtimeContextSchema(candidate.runtimeContextSchema)
    )
    || (
      candidate.targetHierarchy !== undefined
      && !stringArray(candidate.targetHierarchy)
    )
    || (
      candidate.signals !== undefined
      && !signalReferences(candidate.signals)
    )
    || (
      candidate.inference !== undefined
      && !inference(candidate.inference)
    )
    || (
      candidate.onlineStrategy !== undefined
      && !onlineStrategy(candidate.onlineStrategy)
    )
  ) {
    return false;
  }

  const actionSpace = record(candidate.actionSpace);
  const fallback = record(candidate.fallback);
  return actionSpace !== undefined
    && exactKeys(
      actionSpace,
      ["type", "min", "max", "default"],
      ["step"],
    )
    && actionSpace.type === "number"
    && finite(actionSpace.min)
    && finite(actionSpace.max)
    && finite(actionSpace.default)
    && actionSpace.min <= actionSpace.max
    && actionSpace.default >= actionSpace.min
    && actionSpace.default <= actionSpace.max
    && (
      actionSpace.step === undefined
      || (finite(actionSpace.step) && actionSpace.step > 0)
    )
    && fallback !== undefined
    && exactKeys(fallback, ["value"], ["reason"])
    && finite(fallback.value)
    && fallback.value >= actionSpace.min
    && fallback.value <= actionSpace.max
    && (
      actionSpace.step === undefined
      || (
        stepAligned(actionSpace.default, actionSpace.min, actionSpace.step)
        && stepAligned(fallback.value, actionSpace.min, actionSpace.step)
      )
    )
    && optionalString(fallback.reason);
}
