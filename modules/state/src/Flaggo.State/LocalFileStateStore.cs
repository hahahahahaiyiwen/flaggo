using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed record LocalFileStateStoreOptions(string FilePath);

public sealed partial class LocalFileStateStore(
    LocalFileStateStoreOptions options) : IStateStore, IStateHealth
{
    private const int FormatVersion = 1;

    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    private readonly string _filePath = Path.GetFullPath(options.FilePath);

    public async Task<GovernedDecisionState?> GetActiveAsync(
        string decisionKey,
        string definitionId,
        string revision,
        IReadOnlyList<DecisionTargetRef?> resolutionTargets,
        CancellationToken cancellationToken)
    {
        var states = await LoadAsync(cancellationToken);
        return resolutionTargets
            .Select(target => states.FirstOrDefault(item =>
                string.Equals(item.DecisionKey, decisionKey, StringComparison.Ordinal) &&
                string.Equals(
                    item.State.DefinitionId,
                    definitionId,
                    StringComparison.Ordinal) &&
                string.Equals(item.State.Revision, revision, StringComparison.Ordinal) &&
                TargetsEqual(item.State.ControlTarget, target)).State)
            .FirstOrDefault(candidate => candidate is not null);
    }

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        try
        {
            await LoadAsync(cancellationToken);
            return true;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch
        {
            return false;
        }
    }

    private async Task<IReadOnlyList<(string DecisionKey, GovernedDecisionState State)>>
        LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            await using var stream = new FileStream(
                _filePath,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete,
                4096,
                FileOptions.Asynchronous | FileOptions.SequentialScan);
            var document = await JsonSerializer.DeserializeAsync<PersistedStateDocument>(
                stream,
                JsonOptions,
                cancellationToken);
            if (document is null)
            {
                throw new InvalidDataException("The local governed-state file is empty.");
            }

            return ValidateAndMap(document);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The local governed-state file is not valid strict JSON.",
                error);
        }
    }

    private static IReadOnlyList<(string DecisionKey, GovernedDecisionState State)>
        ValidateAndMap(PersistedStateDocument document)
    {
        if (document.Version != FormatVersion || document.States is null)
        {
            throw new InvalidDataException(
                $"The local governed-state file must use version {FormatVersion}.");
        }

        var states = new List<(string DecisionKey, GovernedDecisionState State)>();
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var persisted in document.States)
        {
            if (persisted is null ||
                string.IsNullOrWhiteSpace(persisted.DecisionKey) ||
                string.IsNullOrWhiteSpace(persisted.DefinitionId) ||
                string.IsNullOrWhiteSpace(persisted.Revision) ||
                !Sha256DigestPattern().IsMatch(persisted.ContractDigest ?? string.Empty) ||
                persisted.Value is not JsonElement value ||
                !IsDecisionValue(value) ||
                string.IsNullOrWhiteSpace(persisted.Mode))
            {
                throw new InvalidDataException(
                    "A local governed-state entry is missing required identity or value data.");
            }

            var controlTarget = ValidateAndMapTarget(persisted.ControlTarget);
            var numericRule = ValidateAndMapNumericRule(persisted.NumericRule);
            ValidateMode(persisted, value, numericRule);
            var targetIdentity = controlTarget is null
                ? "global:null"
                : $"{controlTarget.Type}:{controlTarget.Id}";
            if (!keys.Add(
                    $"{persisted.DecisionKey}\n{persisted.DefinitionId}\n" +
                    $"{persisted.Revision}\n{targetIdentity}"))
            {
                throw new InvalidDataException(
                    "The local governed-state file contains a duplicate state identity.");
            }

            states.Add(
                (
                    persisted.DecisionKey,
                    new GovernedDecisionState(
                        persisted.DefinitionId,
                        persisted.Revision,
                        persisted.ContractDigest!,
                        value.Clone(),
                        controlTarget,
                        persisted.Mode,
                        persisted.StrategyId,
                        numericRule,
                        persisted.LastChangedAt)));
        }

        return states;
    }

    private static void ValidateMode(
        PersistedState state,
        JsonElement value,
        NumericRuleStrategy? numericRule)
    {
        switch (state.Mode)
        {
            case "active-value":
                if (state.StrategyId is not null || numericRule is not null)
                {
                    throw new InvalidDataException(
                        "An active-value state cannot carry strategy configuration.");
                }

                return;
            case "strategy":
                if (string.IsNullOrWhiteSpace(state.StrategyId) ||
                    numericRule is null ||
                    value.ValueKind != JsonValueKind.Number ||
                    !value.TryGetDouble(out var currentValue) ||
                    !double.IsFinite(currentValue))
                {
                    throw new InvalidDataException(
                        "A strategy state requires a strategy id, numeric current value, and numeric rule.");
                }

                return;
            case "experiment":
            case "fallback":
                throw new InvalidDataException(
                    $"Persisted decision mode '{state.Mode}' is not supported by the local state adapter.");
            default:
                throw new InvalidDataException(
                    "A local governed-state entry contains an unknown decision mode.");
        }
    }

    private static DecisionTargetRef? ValidateAndMapTarget(
        PersistedDecisionTarget? target)
    {
        if (target is null)
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(target.Type) ||
            string.IsNullOrWhiteSpace(target.Id))
        {
            throw new InvalidDataException(
                "A local governed-state target must contain type and id.");
        }

        return new DecisionTargetRef(target.Type, target.Id);
    }

    private static NumericRuleStrategy? ValidateAndMapNumericRule(
        PersistedNumericRule? rule)
    {
        if (rule is null)
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(rule.InputSignalKey) ||
            rule.Threshold is not double threshold ||
            !double.IsFinite(threshold) ||
            rule.ValueAtOrAbove is not double valueAtOrAbove ||
            !double.IsFinite(valueAtOrAbove) ||
            rule.ValueBelow is not double valueBelow ||
            !double.IsFinite(valueBelow))
        {
            throw new InvalidDataException(
                "A local numeric rule contains invalid scalar configuration.");
        }

        if (rule.WeightedInputs is not { Count: > 0 } weightedInputs)
        {
            return new NumericRuleStrategy(
                rule.InputSignalKey,
                threshold,
                valueAtOrAbove,
                valueBelow,
                rule.WeightedInputs is null ? null : []);
        }

        var keys = new HashSet<string>(StringComparer.Ordinal);
        var totalWeight = 0d;
        var mappedInputs = new List<NumericRuleInput>(weightedInputs.Count);
        foreach (var input in weightedInputs)
        {
            if (input is null ||
                string.IsNullOrWhiteSpace(input.SignalKey) ||
                !keys.Add(input.SignalKey) ||
                input.Minimum is not double minimum ||
                !double.IsFinite(minimum) ||
                input.Maximum is not double maximum ||
                !double.IsFinite(maximum) ||
                maximum <= minimum ||
                input.Weight is not double weight ||
                !double.IsFinite(weight) ||
                weight < 0)
            {
                throw new InvalidDataException(
                    "A local weighted numeric rule contains invalid input configuration.");
            }

            totalWeight += weight;
            mappedInputs.Add(
                new NumericRuleInput(input.SignalKey, minimum, maximum, weight));
        }

        if (!double.IsFinite(totalWeight) || totalWeight <= 0)
        {
            throw new InvalidDataException(
                "A local weighted numeric rule must have positive total weight.");
        }

        return new NumericRuleStrategy(
            rule.InputSignalKey,
            threshold,
            valueAtOrAbove,
            valueBelow,
            mappedInputs);
    }

    private static bool IsDecisionValue(JsonElement value) =>
        (value.ValueKind is
            JsonValueKind.True or
            JsonValueKind.False or
            JsonValueKind.String) ||
        value.ValueKind == JsonValueKind.Number &&
        value.TryGetDouble(out var number) &&
        double.IsFinite(number);

    private static bool TargetsEqual(DecisionTargetRef? left, DecisionTargetRef? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        string.Equals(left.Type, right.Type, StringComparison.Ordinal) &&
        string.Equals(left.Id, right.Id, StringComparison.Ordinal);

    [GeneratedRegex("^sha256:[0-9a-f]{64}$", RegexOptions.CultureInvariant)]
    private static partial Regex Sha256DigestPattern();

    private sealed record PersistedStateDocument(
        int? Version,
        IReadOnlyList<PersistedState?>? States);

    private sealed record PersistedState(
        string? DecisionKey,
        string? DefinitionId,
        string? Revision,
        string? ContractDigest,
        JsonElement? Value,
        PersistedDecisionTarget? ControlTarget,
        string? Mode,
        string? StrategyId,
        PersistedNumericRule? NumericRule,
        DateTimeOffset? LastChangedAt);

    private sealed record PersistedDecisionTarget(
        string? Type,
        string? Id);

    private sealed record PersistedNumericRule(
        string? InputSignalKey,
        double? Threshold,
        double? ValueAtOrAbove,
        double? ValueBelow,
        IReadOnlyList<PersistedNumericRuleInput?>? WeightedInputs);

    private sealed record PersistedNumericRuleInput(
        string? SignalKey,
        double? Minimum,
        double? Maximum,
        double? Weight);
}
