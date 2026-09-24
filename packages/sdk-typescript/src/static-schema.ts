import { FlaggoError } from "./errors.js";
import type {
  ContractIssue,
  DecisionDefinitionBundle,
  DecisionValue,
  NumberActionSpace,
  PrimitiveType,
  RuntimeCatalog,
} from "./types.js";

export class ManifestValidationError extends FlaggoError {
  readonly issues: ContractIssue[];

  constructor(path: string, message: string, code = "invalid-definition") {
    super(`${path}: ${message}`);
    this.name = "ManifestValidationError";
    this.issues = [{ code, severity: "error", path, message }];
  }
}

export function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function check(condition: unknown, path: string, message: string): asserts condition {
  if (!condition) throw new ManifestValidationError(path, message);
}

function object(value: unknown, path: string): Record<string, unknown> {
  const candidate = record(value);
  check(candidate, path, "An object is required.");
  return candidate;
}

function keys(
  value: Record<string, unknown>,
  path: string,
  required: readonly string[],
  optional: readonly string[] = [],
): void {
  const allowed = new Set([...required, ...optional]);
  check(required.every((key) => Object.hasOwn(value, key)), path, "Required fields are missing.");
  for (const key of Object.keys(value)) {
    check(allowed.has(key), `${path}/${pointer(key)}`, "Unknown field.");
  }
}

export function pointer(value: string): string {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function text(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

export function primitive(value: unknown): value is DecisionValue {
  return typeof value === "string" || typeof value === "boolean" || finite(value);
}

function stringList(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(text) && new Set(value).size === value.length;
}

function primitiveType(value: unknown): boolean {
  return value === "number" || value === "boolean" || value === "string";
}

function decimalValue(value: number): { coefficient: bigint; scale: number } {
  const [mantissa, exponentText] = value.toString().toLowerCase().split("e");
  const [whole, fraction = ""] = mantissa!.split(".");
  let coefficient = BigInt(`${whole}${fraction}`);
  let scale = fraction.length - Number(exponentText ?? 0);
  if (scale < 0) {
    coefficient *= 10n ** BigInt(-scale);
    scale = 0;
  }
  return { coefficient, scale };
}

export function stepAligned(value: number, minimum: number, step: number): boolean {
  const values = [value, minimum, step].map(decimalValue);
  const scale = Math.max(...values.map((item) => item.scale));
  const scaled = values.map((item) => item.coefficient * 10n ** BigInt(scale - item.scale));
  return (scaled[0]! - scaled[1]!) % scaled[2]! === 0n;
}

function inputSchema(value: Record<string, unknown>, path: string): void {
  check(primitiveType(value.type), `${path}/type`, "Expected a primitive type.");
  if (value.type !== "number") {
    check(value.unit === undefined && value.range === undefined, path, "Only numeric values have units/ranges.");
    return;
  }
  check(value.unit === undefined || typeof value.unit === "string", path, "Unit must be a string.");
  if (value.range !== undefined) {
    check(Array.isArray(value.range) && value.range.length === 2
      && finite(value.range[0]) && finite(value.range[1]) && value.range[0] <= value.range[1],
    `${path}/range`, "Expected finite inclusive [minimum, maximum] bounds.");
  }
}

export function inputMatches(
  value: unknown,
  schema: { type: PrimitiveType; range?: readonly [number, number] },
): value is DecisionValue {
  if (schema.type === "number") {
    return finite(value) && (schema.range === undefined
      || (value >= schema.range[0] && value <= schema.range[1]));
  }
  return typeof value === schema.type;
}

function result(value: unknown, path: string): void {
  const candidate = object(value, path);
  check(primitiveType(candidate.type), path, "Unknown result type.");
  if (candidate.type === "number") {
    keys(candidate, path, ["type", "min", "max", "default"], ["step"]);
    check(finite(candidate.min) && finite(candidate.max) && finite(candidate.default)
      && candidate.min <= candidate.max
      && candidate.default >= candidate.min && candidate.default <= candidate.max,
    path, "The finite default must be within ordered result bounds.");
    if (candidate.step !== undefined) {
      check(finite(candidate.step) && candidate.step > 0
        && stepAligned(candidate.default, candidate.min, candidate.step),
      path, "The positive step must align the default with the minimum.");
    }
  } else if (candidate.type === "boolean") {
    keys(candidate, path, ["type", "default"]);
    check(typeof candidate.default === "boolean", path, "Expected a boolean default.");
  } else {
    keys(candidate, path, ["type", "default"], ["allowedValues"]);
    check(typeof candidate.default === "string", path, "Expected a string default.");
    if (candidate.allowedValues !== undefined) {
      check(Array.isArray(candidate.allowedValues)
        && candidate.allowedValues.length > 0
        && candidate.allowedValues.every((item) => typeof item === "string")
        && new Set(candidate.allowedValues).size === candidate.allowedValues.length
        && candidate.allowedValues.includes(candidate.default),
      path, "The default must be one of the unique allowed string values.");
    }
  }
}

function attributes(value: unknown, path: string): void {
  check(Object.entries(object(value, path)).every(([key, item]) => text(key) && primitive(item)),
    path, "Attribute filters must be exact primitive values with nonempty keys.");
}

function attributeSelector(value: unknown, path: string, events: boolean): void {
  const candidate = object(value, path);
  keys(candidate, path, ["from", "key"]);
  check(text(candidate.key) && (candidate.from === "resourceAttributes"
    || candidate.from === "attributes" || (events && candidate.from === "eventAttributes")),
  path, "Unsupported attribute namespace or empty key.");
}

function binding(value: unknown, path: string): void {
  const candidate = object(value, path);
  keys(candidate, path,
    ["meaning", "type", "source", "projection", "target", "freshness", "sampling", "attribution"],
    ["unit", "range"]);
  inputSchema(candidate, path);
  check(text(candidate.meaning), path, "Evidence meaning is required.");
  const source = object(candidate.source, `${path}/source`);
  const event = source.kind === "span" && source.eventName !== undefined;
  const common = ["scope", "resourceAttributes", "value", "kind"];
  if (source.kind === "metric") {
    keys(source, `${path}/source`, [...common, "name", "dataType"], ["attributes"]);
    check(source.dataType === "gauge" && text(source.name) && candidate.type === "number"
      && typeof candidate.unit === "string", path, "Metrics require a numeric Gauge and exact unit.");
  } else if (source.kind === "span") {
    keys(source, `${path}/source`, [...common, "name"],
      event ? ["attributes", "eventName", "eventAttributes"] : ["attributes"]);
    check(text(source.name) && (!event || text(source.eventName)), path, "Span names must be nonempty.");
  } else {
    check(source.kind === "log", `${path}/source/kind`, "Unsupported telemetry source.");
    keys(source, `${path}/source`, common, ["attributes", "eventName", "bodyEquals"]);
    check((text(source.eventName) && !Object.hasOwn(source, "bodyEquals"))
      || (!Object.hasOwn(source, "eventName") && primitive(source.bodyEquals)),
    path, "Logs select either an exact event name or a scalar body.");
  }
  const scope = object(source.scope, `${path}/source/scope`);
  keys(scope, `${path}/source/scope`, ["name"], ["version"]);
  check(text(scope.name) && (scope.version === undefined || typeof scope.version === "string"),
    path, "An exact instrumentation scope is required.");
  attributes(source.resourceAttributes, `${path}/source/resourceAttributes`);
  if (source.attributes !== undefined) attributes(source.attributes, `${path}/source/attributes`);
  if (source.eventAttributes !== undefined) attributes(source.eventAttributes, `${path}/source/eventAttributes`);
  const selected = object(source.value, `${path}/source/value`);
  if (source.kind === "metric") {
    keys(selected, path, ["from"]);
    check(selected.from === "value", path, "A Gauge selects its point value.");
  } else if (source.kind === "span" && !event && selected.from === "duration") {
    keys(selected, path, ["from"]);
    check(candidate.type === "number" && candidate.unit === "ms", path, "Span duration must be numeric milliseconds.");
  } else if (source.kind === "log" && selected.from === "body") {
    keys(selected, path, ["from", "path"]);
    check(Array.isArray(selected.path) && selected.path.every((item) => typeof item === "string"),
      path, "Log body paths are literal object-key arrays.");
  } else {
    keys(selected, path, ["from", "key"]);
    check(text(selected.key) && selected.from === (event ? "eventAttributes" : "attributes"),
      path, "Select a primitive attribute from the appropriate record.");
  }
  const projection = object(candidate.projection, `${path}/projection`);
  keys(projection, path, ["kind"]);
  check(projection.kind === "latest", path, "Only latest scalar projections are supported.");
  const target = object(candidate.target, `${path}/target`);
  keys(target, path, target.type === "global" ? ["type"] : ["type", "idAttribute"]);
  check(text(target.type), path, "Target type is required.");
  if (target.type !== "global") attributeSelector(target.idAttribute, `${path}/target/idAttribute`, event);
  const freshness = object(candidate.freshness, `${path}/freshness`);
  keys(freshness, path, ["maxAgeSeconds"]);
  check(finite(freshness.maxAgeSeconds) && Number.isSafeInteger(freshness.maxAgeSeconds)
    && freshness.maxAgeSeconds > 0 && freshness.maxAgeSeconds <= 922337203685,
  path, "Freshness must be a positive, representable integer duration.");
  const sampling = object(candidate.sampling, `${path}/sampling`);
  keys(sampling, path, ["accept"]);
  check(sampling.accept === "observed", path, "Only observed coverage is supported; no sampling correction is inferred.");
  const attribution = object(candidate.attribution, `${path}/attribution`);
  if (attribution.kind === "none") {
    keys(attribution, path, ["kind"]);
  } else {
    keys(attribution, path, ["kind", "exposureIdAttribute"]);
    check(attribution.kind === "confirmed-exposure" && source.kind !== "metric",
      path, "Confirmed exposure attribution is supported only for spans and logs.");
    attributeSelector(attribution.exposureIdAttribute, `${path}/attribution/exposureIdAttribute`, event);
  }
}

function context(value: unknown, path: string): Record<string, unknown> {
  const candidate = object(value, path);
  const targets = new Set<string>();
  for (const [name, field] of Object.entries(candidate)) {
    const item = object(field, `${path}/${pointer(name)}`);
    keys(item, path, ["type"], ["required", "target"]);
    check(text(name) && primitiveType(item.type) && (item.required === undefined || typeof item.required === "boolean"),
      path, "Context fields need names, primitive types and boolean requiredness.");
    if (item.target !== undefined) {
      check(text(item.target) && item.target !== "global" && item.type === "string" && !targets.has(item.target),
        path, "Each non-global target has exactly one string context source.");
      targets.add(item.target);
    }
  }
  return candidate;
}

function inputs(value: unknown, path: string): Record<string, unknown> {
  const candidate = object(value, path);
  for (const [name, input] of Object.entries(candidate)) {
    const item = object(input, `${path}/${pointer(name)}`);
    check(text(name), path, "Input names cannot be empty.");
    if (item.source === "request") {
      keys(item, path, ["source", "type", "meaning"], ["unit", "range"]);
      check(text(item.meaning), path, "Request inputs require semantic meaning.");
      inputSchema(item, path);
    } else {
      keys(item, path, ["source", "binding"]);
      check(item.source === "evidence" && text(item.binding), path, "Evidence inputs require an explicit binding.");
    }
  }
  return candidate;
}

function policy(value: unknown, path: string): void {
  const candidate = object(value, path);
  if (candidate.kind === "reference") {
    keys(candidate, path, ["kind", "policyId"]);
    check(text(candidate.policyId), path, "Policy ID is required.");
    return;
  }
  keys(candidate, path, ["kind", "constraints"], ["clientFallback"]);
  check(candidate.kind === "inline" && Array.isArray(candidate.constraints), path, "Expected explicit inline constraints.");
  const kinds = new Set<string>();
  for (const value of candidate.constraints) {
    const item = object(value, path);
    check(text(item.kind) && !kinds.has(item.kind), path, "Constraint kinds must be unique.");
    kinds.add(item.kind);
    if (item.kind === "number-bounds") {
      keys(item, path, ["kind", "min", "max"]);
      check(finite(item.min) && finite(item.max) && item.min <= item.max, path, "Invalid policy bounds.");
    } else if (item.kind === "pause") {
      keys(item, path, ["kind", "paused"]);
      check(typeof item.paused === "boolean", path, "Pause must be boolean.");
    } else if (item.kind === "cooldown") {
      keys(item, path, ["kind", "seconds"]);
      check(finite(item.seconds) && item.seconds >= 0, path, "Cooldown must be nonnegative.");
    } else {
      keys(item, path, ["kind", "value"]);
      check(["max-delta", "min-evidence-quality", "max-model-uncertainty", "min-expected-outcome", "min-sample-size"].includes(item.kind)
        && finite(item.value) && item.value >= 0, path, "Unsupported or invalid constraint.");
      if (!["max-delta", "min-sample-size"].includes(item.kind)) {
        check(item.value <= 1, path, "Quality constraints must be in [0, 1].");
      }
    }
  }
  if (candidate.clientFallback !== undefined) {
    const fallback = object(candidate.clientFallback, path);
    keys(fallback, path, ["requiredEvidenceUnavailable"]);
    check(["allow", "forbid"].includes(String(fallback.requiredEvidenceUnavailable)), path, "Invalid availability fallback rule.");
  }
}

export function assertManifest(value: unknown): asserts value is DecisionDefinitionBundle {
  const manifest = object(value, "/");
  keys(manifest, "/", ["format", "application", "decisions"], ["source", "build"]);
  check(manifest.format === "flaggo.decision-definition-bundle/v2", "/format", "Only manifest v2 is supported.");
  const application = object(manifest.application, "/application");
  keys(application, "/application", ["id", "environment"]);
  check(text(application.id) && text(application.environment), "/application", "Application scope is required.");
  for (const [name, fields] of [["source", ["repository", "path", "commit"]], ["build", ["buildId", "artifactDigest", "version"]]] as const) {
    if (manifest[name] === undefined) continue;
    const metadata = object(manifest[name], `/${name}`);
    keys(metadata, `/${name}`, [], fields);
    check(Object.values(metadata).every((item) => typeof item === "string"), `/${name}`, "Metadata values must be strings.");
  }
  const decisions = object(manifest.decisions, "/decisions");
  check(Object.keys(decisions).length > 0, "/decisions", "At least one decision is required.");
  for (const [name, value] of Object.entries(decisions)) {
    const path = `/decisions/${pointer(name)}`;
    const definition = object(value, path);
    check(text(name), path, "Decision keys cannot be empty.");
    keys(definition, path, ["result", "targeting", "policy"], ["context", "inputs", "evidence", "intent", "owner"]);
    check(definition.owner === undefined || typeof definition.owner === "string", path, "Owner must be a string.");
    result(definition.result, `${path}/result`);
    policy(definition.policy, `${path}/policy`);
    const fields = context(definition.context ?? {}, `${path}/context`);
    const operands = inputs(definition.inputs ?? {}, `${path}/inputs`);
    const targeting = object(definition.targeting, `${path}/targeting`);
    keys(targeting, path, ["hierarchy", "primary", "fallbackOrder"]);
    check(stringList(targeting.hierarchy) && targeting.hierarchy.length > 0
      && text(targeting.primary) && targeting.hierarchy.includes(targeting.primary)
      && stringList(targeting.fallbackOrder)
      && targeting.fallbackOrder.every((target) => targeting.hierarchy instanceof Array
        && targeting.hierarchy.includes(target) && target !== targeting.primary),
    path, "Targets and ordered unique fallbacks must belong to the explicit hierarchy.");
    for (const field of Object.values(fields)) {
      const item = object(field, path);
      check(item.target === undefined || targeting.hierarchy.includes(String(item.target)), path, "Context target is outside the hierarchy.");
    }
    const evidence = object(definition.evidence ?? {}, `${path}/evidence`);
    for (const [key, value] of Object.entries(evidence)) {
      check(text(key), path, "Binding keys cannot be empty.");
      binding(value, `${path}/evidence/${pointer(key)}`);
      const target = object(object(value, path).target, path);
      check(targeting.hierarchy.includes(String(target.type)), path, "Evidence target is outside the hierarchy.");
    }
    for (const [key, input] of Object.entries(operands)) {
      const item = object(input, path);
      if (item.source !== "evidence") continue;
      check(typeof item.binding === "string" && Object.hasOwn(evidence, item.binding),
        `${path}/inputs/${pointer(key)}`, "Unknown evidence binding.");
      const selectedBinding = object(evidence[item.binding], path);
      check(object(selectedBinding.attribution, path).kind === "none",
        `${path}/inputs/${pointer(key)}`,
        "Confirmed-exposure bindings are outcome evidence, not required runtime inputs: the first decision cannot create its own prerequisite exposure.");
      const target = object(selectedBinding.target, path);
      check(target.type === "global" || Object.values(fields).some((field) => {
        const candidate = object(field, path);
        return candidate.target === target.type && candidate.required === true;
      }), path, "Required evidence targets require caller context.");
    }
    if (definition.intent !== undefined) {
      const intent = object(definition.intent, `${path}/intent`);
      if (intent.type === "natural-language") {
        keys(intent, path, ["type", "text"]);
        check(text(intent.text), path, "Intent text is required.");
      } else {
        keys(intent, path, ["type", "primary"], ["secondary", "rationale"]);
        check(intent.type === "numeric-objective" && (intent.secondary === undefined || Array.isArray(intent.secondary))
          && (intent.rationale === undefined || typeof intent.rationale === "string"), path, "Unsupported intent.");
        for (const value of [intent.primary, ...(Array.isArray(intent.secondary) ? intent.secondary : [])]) {
          const objective = object(value, path);
          keys(objective, path, ["evidence", "direction"], objective.direction === "target" ? ["target"] : []);
          check(typeof objective.evidence === "string" && Object.hasOwn(evidence, objective.evidence)
            && object(evidence[objective.evidence], path).type === "number"
            && (objective.direction === "minimize" || objective.direction === "maximize"
              || (objective.direction === "target" && finite(objective.target))),
          path, "Numeric objectives require numeric bindings and a supported direction.");
        }
      }
    }
  }
}

export function assertCatalog(value: unknown): asserts value is RuntimeCatalog {
  const catalog = object(value, "/catalog");
  keys(catalog, "/catalog", ["format", "application", "bundleDigest", "decisions"]);
  const digest = (value: unknown): boolean => typeof value === "string" && /^sha256:[0-9a-f]{64}$/u.test(value);
  check(catalog.format === "flaggo.runtime-catalog/v1" && digest(catalog.bundleDigest), "/catalog", "Invalid generated catalog identity.");
  const application = object(catalog.application, "/catalog/application");
  keys(application, "/catalog/application", ["id", "environment"]);
  check(text(application.id) && text(application.environment), "/catalog/application", "Application scope is required.");
  const decisions = object(catalog.decisions, "/catalog/decisions");
  check(Object.keys(decisions).length > 0, "/catalog/decisions", "The catalog cannot be empty.");
  for (const [key, value] of Object.entries(decisions)) {
    const path = `/catalog/decisions/${pointer(key)}`;
    const decision = object(value, path);
    keys(decision, path, ["result", "context", "inputs", "contractDigest"]);
    check(text(key) && digest(decision.contractDigest), path, "Invalid catalog decision identity.");
    result(decision.result, path);
    context(decision.context, path);
    inputs(decision.inputs, path);
  }
}

export function isNumberResult(value: unknown): value is NumberActionSpace {
  try {
    result(value, "/result");
    return record(value)?.type === "number";
  } catch (error) {
    if (error instanceof ManifestValidationError) return false;
    throw error;
  }
}
