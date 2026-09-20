using System.Text.Json;

namespace Flaggo.Shared.Contracts;

public sealed class DecisionProposalValidationException(
    string code,
    string message) : Exception(message)
{
    public string Code { get; } = code;
}

public static class DecisionProposalValidation
{
    public static void Validate(DecisionProposal proposal)
    {
        if (proposal?.Context is not { } context ||
            context.Source is null ||
            context.Definition is not { Contract: not null } definition ||
            context.ExpectedBaseline is not { } baseline ||
            context.EvidenceReferences is null ||
            context.ConfidenceReferences is null)
        {
            throw Invalid("The proposal is missing required typed context.");
        }

        if (string.IsNullOrWhiteSpace(context.ProposalId) ||
            !Enum.IsDefined(context.Source.Kind) ||
            string.IsNullOrWhiteSpace(context.Source.Reference) ||
            string.IsNullOrWhiteSpace(context.Rationale) ||
            string.IsNullOrWhiteSpace(definition.AppId) ||
            string.IsNullOrWhiteSpace(definition.Environment) ||
            string.IsNullOrWhiteSpace(definition.DecisionKey) ||
            string.IsNullOrWhiteSpace(definition.Contract.DefinitionId) ||
            string.IsNullOrWhiteSpace(definition.Contract.Revision) ||
            !LifecycleJson.IsDigest(definition.Contract.ContractDigest) ||
            definition.Contract.BundleDigest is not null && !LifecycleJson.IsDigest(definition.Contract.BundleDigest))
        {
            throw Invalid("The proposal must carry complete identity, source, and rationale.");
        }

        if (context.ControlTarget is { } target &&
            (string.IsNullOrWhiteSpace(target.Type) ||
             string.IsNullOrWhiteSpace(target.Id)) ||
            baseline.StateId is null && baseline.Generation != 0 ||
            baseline.StateId is not null &&
            (string.IsNullOrWhiteSpace(baseline.StateId) || baseline.Generation <= 0))
        {
            throw Invalid("The proposal target or expected baseline is invalid.");
        }

        foreach (var references in new[]
                 {
                     context.EvidenceReferences,
                     context.ConfidenceReferences
                 })
        {
            if (references.Any(string.IsNullOrWhiteSpace) ||
                references.Distinct(StringComparer.Ordinal).Count() != references.Count)
            {
                throw Invalid("Proposal evidence and confidence references must be nonempty and unique.");
            }
        }

        if (context.CreatedAt == default ||
            context.ExpiresAt is { } expiry && expiry <= context.CreatedAt)
        {
            throw Invalid("Proposal creation and expiry metadata is invalid.");
        }

        switch (proposal)
        {
            case FixedValueDecisionProposal fixedValue:
                ValidateValue(fixedValue.Value);
                break;
            case NumericStrategyDecisionProposal strategy:
                ValidateValue(strategy.InitialValue);
                if (strategy.InitialValue.ValueKind != JsonValueKind.Number ||
                    string.IsNullOrWhiteSpace(strategy.StrategyId) ||
                    strategy.Strategy is null)
                {
                    throw Invalid("A numeric strategy requires a numeric initial value and strategy identity.");
                }

                ValidateStrategy(strategy.Strategy);
                break;
            default:
                throw new DecisionProposalValidationException(
                    "unsupported-state-kind",
                    "The decision proposal kind is not supported.");
        }
    }

    public static void ValidateValue(JsonElement value)
    {
        if (value.ValueKind is JsonValueKind.True or JsonValueKind.False or JsonValueKind.String ||
            value.ValueKind == JsonValueKind.Number &&
            CanonicalJson.IsIeee754CompatibleNumber(value))
        {
            return;
        }

        throw Invalid("A governed value must be a canonical primitive decision value.");
    }

    public static void ValidateStrategy(NumericRuleStrategy strategy)
    {
        if (string.IsNullOrWhiteSpace(strategy.InputSignalKey) ||
            !double.IsFinite(strategy.Threshold) ||
            !double.IsFinite(strategy.ValueAtOrAbove) ||
            !double.IsFinite(strategy.ValueBelow))
        {
            throw Invalid("A numeric strategy contains invalid scalar configuration.");
        }

        if (strategy.WeightedInputs is not { Count: > 0 } inputs)
        {
            return;
        }

        var keys = new HashSet<string>(StringComparer.Ordinal);
        var totalWeight = 0d;
        foreach (var input in inputs)
        {
            if (input is null ||
                string.IsNullOrWhiteSpace(input.SignalKey) ||
                !keys.Add(input.SignalKey) ||
                !double.IsFinite(input.Minimum) ||
                !double.IsFinite(input.Maximum) ||
                input.Maximum <= input.Minimum ||
                !double.IsFinite(input.Weight) ||
                input.Weight < 0)
            {
                throw Invalid("A numeric strategy contains an invalid weighted input.");
            }

            totalWeight += input.Weight;
        }

        if (!double.IsFinite(totalWeight) || totalWeight <= 0)
        {
            throw Invalid("A numeric strategy must have positive total weight.");
        }
    }

    private static DecisionProposalValidationException Invalid(string message) =>
        new("invalid-proposal", message);
}
