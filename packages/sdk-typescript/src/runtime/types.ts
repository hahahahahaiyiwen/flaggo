import type {
  CurrentExposure as GeneratedCurrentExposure,
  EvaluationProvenance as GeneratedEvaluationProvenance,
  RuntimeDecision as GeneratedRuntimeDecision,
} from "../generated/runtime-models.generated.js";
import type {
  DeepReadonly,
  FlaggoResponse,
  JsonValue,
  RequestOptions,
  Sha256Digest,
  TransportConfiguration,
} from "../shared/types.js";

export interface DecisionSpec<
  TAttributes extends Readonly<Record<string, JsonValue>> =
    Readonly<Record<string, JsonValue>>,
  TResult extends JsonValue = JsonValue,
> {
  readonly attributes: TAttributes;
  readonly result: TResult;
}

export type DecisionCatalog =
  Readonly<Record<
    string,
    DecisionSpec<Readonly<Record<string, JsonValue>>, JsonValue>
  >>;

export interface DecisionBinding {
  readonly contractDigest: Sha256Digest;
}

export type DecisionBindings<TCatalog extends DecisionCatalog> = Readonly<{
  [TName in keyof TCatalog]: DecisionBinding;
}>;

export type CurrentExposure = DeepReadonly<GeneratedCurrentExposure>;
export type RuntimeEvaluation =
  DeepReadonly<GeneratedEvaluationProvenance>;

export type RuntimeDecision<TResult extends JsonValue = JsonValue> =
  Omit<
    DeepReadonly<GeneratedRuntimeDecision>,
    "contractDigest" | "executableDigest" | "result"
  > & {
    readonly contractDigest: Sha256Digest;
    readonly executableDigest: Sha256Digest;
    readonly result: TResult;
  };

type AttributesOf<TSpec> =
  TSpec extends DecisionSpec<infer TAttributes, JsonValue>
    ? TAttributes
    : never;

type ResultOf<TSpec> =
  TSpec extends DecisionSpec<Readonly<Record<string, JsonValue>>, infer TResult>
    ? TResult
    : never;

export interface DecisionRequest<
  TSpec extends DecisionSpec<Readonly<Record<string, JsonValue>>, JsonValue>,
> {
  readonly attributes?: DeepReadonly<Partial<AttributesOf<TSpec>>>;
  readonly currentExposure?: CurrentExposure;
}

export interface DecisionClientConfiguration<TCatalog extends DecisionCatalog>
  extends TransportConfiguration {
  readonly bindings: DecisionBindings<TCatalog>;
  readonly random?: () => number;
}

export interface DecisionClient<TCatalog extends DecisionCatalog> {
  decide<TName extends Extract<keyof TCatalog, string>>(
    contractName: TName,
    request?: DecisionRequest<TCatalog[TName]>,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<RuntimeDecision<ResultOf<TCatalog[TName]>>>>;
}

export function defineDecisionBindings<TCatalog extends DecisionCatalog>(
  bindings: DecisionBindings<TCatalog>,
): DecisionBindings<TCatalog> {
  return bindings;
}
