import type {
  Attribute as GeneratedAttribute,
  AuthoredExecutable as GeneratedAuthoredExecutable,
  AuthoredRule as GeneratedAuthoredRule,
  AuthoredWhen as GeneratedAuthoredWhen,
  DecisionContract as GeneratedDecisionContract,
  DecisionContractVersion as GeneratedDecisionContractVersion,
  DecisionContractVersionList as GeneratedDecisionContractVersionList,
  DecisionContractVersionSummary as GeneratedDecisionContractVersionSummary,
  Evidence as GeneratedEvidence,
  ExpressionReturn as GeneratedExpressionReturn,
  Guardrail as GeneratedGuardrail,
  Learning as GeneratedLearning,
  LearningObjective as GeneratedLearningObjective,
  LearningPolicy as GeneratedLearningPolicy,
  LiteralReturn as GeneratedLiteralReturn,
  Result as GeneratedResult,
  ValidationIssue as GeneratedValidationIssue,
  ValueSchema as GeneratedValueSchema,
} from "../generated/management-models.generated.js";
import type { AuthorityScope } from "../configuration/types.js";
import type {
  DeepReadonly,
  FlaggoResponse,
  JsonValue,
  RequestOptions,
  Sha256Digest,
  TransportConfiguration,
} from "../shared/types.js";

export type ValueSchema = DeepReadonly<GeneratedValueSchema>;
export type ContractAttribute = DeepReadonly<GeneratedAttribute>;
export type RuleWhen = DeepReadonly<GeneratedAuthoredWhen>;
export type ExpressionReturn = DeepReadonly<GeneratedExpressionReturn>;
export type LiteralReturn<TResult extends JsonValue = JsonValue> =
  Omit<DeepReadonly<GeneratedLiteralReturn>, "value"> & {
    readonly value: TResult;
  };
export type RuleReturn<TResult extends JsonValue = JsonValue> =
  | LiteralReturn<TResult>
  | ExpressionReturn;
export type AuthoredRule<TResult extends JsonValue = JsonValue> =
  Omit<DeepReadonly<GeneratedAuthoredRule>, "return"> & {
    readonly return: RuleReturn<TResult>;
  };
export type AuthoredExecutable<TResult extends JsonValue = JsonValue> =
  Omit<DeepReadonly<GeneratedAuthoredExecutable>, "rules"> & {
    readonly rules: readonly AuthoredRule<TResult>[];
  };
export type LearningPolicy = DeepReadonly<GeneratedLearningPolicy>;
export type EvidenceDefinition = DeepReadonly<GeneratedEvidence>;
export type Guardrail = DeepReadonly<GeneratedGuardrail>;
export type LearningObjective = DeepReadonly<GeneratedLearningObjective>;
export type LearningDefinition = DeepReadonly<GeneratedLearning>;

export type ContractResult<TResult extends JsonValue = JsonValue> =
  Omit<DeepReadonly<GeneratedResult>, "default"> & {
    readonly default: TResult;
  };

export type DecisionContract<TResult extends JsonValue = JsonValue> =
  Omit<
    DeepReadonly<GeneratedDecisionContract>,
    "authority" | "result" | "authoredExecutable"
  > & {
    readonly authority: AuthorityScope;
    readonly result: ContractResult<TResult>;
    readonly authoredExecutable?: AuthoredExecutable<TResult>;
  };

export type DecisionContractDefinition<TResult extends JsonValue = JsonValue> =
  Omit<DecisionContract<TResult>, "authority">;

export type ValidationIssue = DeepReadonly<GeneratedValidationIssue>;

export type DecisionContractValidationResult =
  | {
      readonly status: "valid";
      readonly contractDigest: Sha256Digest;
      readonly issues: readonly ValidationIssue[];
    }
  | {
      readonly status: "invalid";
      readonly issues: readonly ValidationIssue[];
    };

export type DecisionContractVersion<TResult extends JsonValue = JsonValue> =
  Omit<
    DeepReadonly<GeneratedDecisionContractVersion>,
    "contractDigest" | "activeExecutableDigest" | "contract"
  > & {
    readonly contractDigest: Sha256Digest;
    readonly activeExecutableDigest: Sha256Digest;
    readonly contract: DecisionContract<TResult>;
  };

export type DecisionContractVersionSummary =
  Omit<
    DeepReadonly<GeneratedDecisionContractVersionSummary>,
    "contractDigest" | "activeExecutableDigest"
  > & {
    readonly contractDigest: Sha256Digest;
    readonly activeExecutableDigest: Sha256Digest;
  };

export type DecisionContractVersionList =
  Omit<
    DeepReadonly<GeneratedDecisionContractVersionList>,
    "currentContractDigest" | "versions"
  > & {
    readonly currentContractDigest: Sha256Digest;
    readonly versions: readonly DecisionContractVersionSummary[];
  };

export interface ListContractVersionsQuery {
  readonly limit?: number;
  readonly cursor?: string;
}

export type ContractClientConfiguration = TransportConfiguration;

export interface ContractClient {
  validate<TResult extends JsonValue>(
    contract: DecisionContract<TResult>,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<DecisionContractValidationResult>>;

  deploy<TResult extends JsonValue>(
    contract: DecisionContract<TResult>,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<DecisionContractVersion<TResult>>>;

  getCurrent<TResult extends JsonValue = JsonValue>(
    contractName: string,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<DecisionContractVersion<TResult>>>;

  getVersion<TResult extends JsonValue = JsonValue>(
    contractName: string,
    contractDigest: Sha256Digest,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<DecisionContractVersion<TResult>>>;

  listVersions(
    contractName: string,
    query?: ListContractVersionsQuery,
    options?: RequestOptions,
  ): Promise<FlaggoResponse<DecisionContractVersionList>>;
}

export function defineDecisionContract<TResult extends JsonValue>(
  contract: DecisionContractDefinition<TResult>,
): DecisionContractDefinition<TResult> {
  return contract;
}

export function bindDecisionContract<TResult extends JsonValue>(
  contract: DecisionContractDefinition<TResult>,
  authority: AuthorityScope,
): DecisionContract<TResult> {
  return {
    ...contract,
    authority: { ...authority },
  };
}
