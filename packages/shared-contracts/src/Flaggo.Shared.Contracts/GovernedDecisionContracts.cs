using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Shared.Contracts;

public sealed record NumericRuleInput(
    string SignalKey,
    double Minimum,
    double Maximum,
    double Weight);

public sealed record NumericRuleStrategy(
    string InputSignalKey,
    double Threshold,
    double ValueAtOrAbove,
    double ValueBelow,
    IReadOnlyList<NumericRuleInput>? WeightedInputs = null);

[JsonConverter(typeof(JsonStringEnumConverter<GovernedDecisionStateStatus>))]
public enum GovernedDecisionStateStatus
{
    Pending,
    Active,
    Superseded,
    Expired,
    Completed,
    RolledBack
}

public sealed record GovernedDecisionState(
    string DefinitionId,
    string Revision,
    string ContractDigest,
    JsonElement Value,
    DecisionTargetRef? ControlTarget = null,
    string Mode = "active-value",
    string? StrategyId = null,
    NumericRuleStrategy? NumericRule = null,
    DateTimeOffset? LastChangedAt = null,
    string? StateId = null,
    string? ProposalId = null,
    long Generation = 0,
    string? PredecessorStateId = null,
    string? ApprovalReference = null,
    DateTimeOffset? ActivatedAt = null,
    GovernedDecisionStateStatus LifecycleStatus =
        GovernedDecisionStateStatus.Active);

[JsonConverter(typeof(JsonStringEnumConverter<DecisionProposalSourceKind>))]
public enum DecisionProposalSourceKind
{
    Operator,
    Scripted,
    Automated,
    Intelligence
}

public sealed record DecisionProposalSource(
    DecisionProposalSourceKind Kind,
    string Reference);

public sealed record GovernedDefinitionIdentity(
    string AppId,
    string Environment,
    string DecisionKey,
    RuntimeContractIdentity Contract);

public sealed record GovernedStateAddress(
    string AppId,
    string Environment,
    string DecisionKey,
    DecisionTargetRef? ControlTarget);

public sealed record GovernedStateBaseline(
    string? StateId,
    long Generation);

public sealed record DecisionProposalContext(
    string ProposalId,
    DecisionProposalSource Source,
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef? ControlTarget,
    GovernedStateBaseline ExpectedBaseline,
    string Rationale,
    IReadOnlyList<string> EvidenceReferences,
    IReadOnlyList<string> ConfidenceReferences,
    DateTimeOffset CreatedAt,
    DateTimeOffset? ExpiresAt);

[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(FixedValueDecisionProposal), "fixed-value")]
[JsonDerivedType(typeof(NumericStrategyDecisionProposal), "numeric-strategy")]
public abstract record DecisionProposal(DecisionProposalContext Context);

public sealed record FixedValueDecisionProposal(
    DecisionProposalContext Context,
    JsonElement Value) : DecisionProposal(Context);

public sealed record NumericStrategyDecisionProposal(
    DecisionProposalContext Context,
    JsonElement InitialValue,
    string StrategyId,
    NumericRuleStrategy Strategy) : DecisionProposal(Context);
