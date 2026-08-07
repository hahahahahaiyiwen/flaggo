using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public interface IStateSnapshotProvider
{
    Task<CommittedArtifactReference> ResolveStateSnapshotAsync(
        CancellationToken cancellationToken);
}

public sealed record LocalFileStateStoreOptions
{
    public LocalFileStateStoreOptions(string commitDescriptorPath)
        : this(CommittedFileSnapshotSource.FromDescriptor(commitDescriptorPath))
    {
    }

    public LocalFileStateStoreOptions(CommittedFileSnapshotSource snapshot)
    {
        ArgumentNullException.ThrowIfNull(snapshot);
        Snapshot = snapshot;
    }

    public CommittedFileSnapshotSource Snapshot { get; }
}

public sealed partial class LocalFileStateStore : IStateStore, IStateHealth
{
    private const int FormatVersion = 1;

    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    private readonly IStateSnapshotProvider _snapshotProvider;
    private readonly CommittedFileSnapshotOptions _snapshotOptions;

    public LocalFileStateStore(LocalFileStateStoreOptions options)
        : this(
            new SourceStateSnapshotProvider(options.Snapshot),
            new CommittedFileSnapshotOptions())
    {
    }

    public LocalFileStateStore(IStateSnapshotProvider snapshotProvider)
        : this(snapshotProvider, new CommittedFileSnapshotOptions())
    {
    }

    internal LocalFileStateStore(
        IStateSnapshotProvider snapshotProvider,
        CommittedFileSnapshotOptions snapshotOptions)
    {
        ArgumentNullException.ThrowIfNull(snapshotProvider);
        ArgumentNullException.ThrowIfNull(snapshotOptions);
        _snapshotProvider = snapshotProvider;
        _snapshotOptions = snapshotOptions;
    }

    public async Task<GovernedDecisionState?> GetActiveAsync(
        string decisionKey,
        string definitionId,
        string revision,
        IReadOnlyList<DecisionTargetRef?> resolutionTargets,
        CancellationToken cancellationToken)
    {
        var states = await LoadAsync(cancellationToken);
        foreach (var target in resolutionTargets)
        {
            if (states.TryGetValue(
                    StateIdentity.Create(
                        decisionKey,
                        definitionId,
                        revision,
                        target),
                    out var state))
            {
                return state;
            }
        }

        return null;
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

    private async Task<IReadOnlyDictionary<StateIdentity, GovernedDecisionState>>
        LoadAsync(CancellationToken cancellationToken)
    {
        try
        {
            var snapshot = await _snapshotProvider.ResolveStateSnapshotAsync(
                cancellationToken);
            var json = await CommittedFileSnapshot.ReadPinnedAsync(
                snapshot,
                _snapshotOptions,
                cancellationToken);
            StrictJson.Validate(json);
            var document = JsonSerializer.Deserialize<PersistedStateDocument>(
                json,
                JsonOptions);
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

    private static IReadOnlyDictionary<StateIdentity, GovernedDecisionState>
        ValidateAndMap(PersistedStateDocument document)
    {
        if (document.Version != FormatVersion || document.States is null)
        {
            throw new InvalidDataException(
                $"The local governed-state file must use version {FormatVersion}.");
        }

        var states = new Dictionary<StateIdentity, GovernedDecisionState>();
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
            var lastChangedAt = ParseLastChangedAt(persisted.LastChangedAt);
            var identity = StateIdentity.Create(
                persisted.DecisionKey,
                persisted.DefinitionId,
                persisted.Revision,
                controlTarget);
            var state = new GovernedDecisionState(
                persisted.DefinitionId,
                persisted.Revision,
                persisted.ContractDigest!,
                value.Clone(),
                controlTarget,
                persisted.Mode,
                persisted.StrategyId,
                numericRule,
                lastChangedAt);
            if (!states.TryAdd(identity, state))
            {
                throw new InvalidDataException(
                    "The local governed-state file contains a duplicate state identity.");
            }
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
                    !CanonicalJson.IsIeee754CompatibleNumber(value))
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
            !TryGetCanonicalDouble(rule.Threshold, out var threshold) ||
            !TryGetCanonicalDouble(rule.ValueAtOrAbove, out var valueAtOrAbove) ||
            !TryGetCanonicalDouble(rule.ValueBelow, out var valueBelow))
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
                !TryGetCanonicalDouble(input.Minimum, out var minimum) ||
                !TryGetCanonicalDouble(input.Maximum, out var maximum) ||
                maximum <= minimum ||
                !TryGetCanonicalDouble(input.Weight, out var weight) ||
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
        CanonicalJson.IsIeee754CompatibleNumber(value);

    private static bool TryGetCanonicalDouble(
        JsonElement? value,
        out double number)
    {
        if (value is JsonElement element &&
            CanonicalJson.IsIeee754CompatibleNumber(element))
        {
            number = element.GetDouble();
            return true;
        }

        number = default;
        return false;
    }

    private static DateTimeOffset? ParseLastChangedAt(string? value)
    {
        if (value is null)
        {
            return null;
        }

        if (!Rfc3339TimestampPattern().IsMatch(value) ||
            !DateTimeOffset.TryParse(
                value,
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out var parsed) ||
            parsed == default)
        {
            throw new InvalidDataException(
                "A local governed-state lastChangedAt must be an RFC 3339 timestamp with an explicit offset.");
        }

        return parsed.ToUniversalTime();
    }

    [GeneratedRegex("\\Asha256:[0-9a-f]{64}\\z", RegexOptions.CultureInvariant)]
    private static partial Regex Sha256DigestPattern();

    [GeneratedRegex(
        "\\A\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}(?:\\.\\d{1,7})?(?:Z|[+-]\\d{2}:\\d{2})\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex Rfc3339TimestampPattern();

    private readonly struct StateIdentity : IEquatable<StateIdentity>
    {
        private StateIdentity(
            string decisionKey,
            string definitionId,
            string revision,
            string? targetType,
            string? targetId)
        {
            DecisionKey = decisionKey;
            DefinitionId = definitionId;
            Revision = revision;
            TargetType = targetType;
            TargetId = targetId;
        }

        private string DecisionKey { get; }
        private string DefinitionId { get; }
        private string Revision { get; }
        private string? TargetType { get; }
        private string? TargetId { get; }

        public static StateIdentity Create(
            string decisionKey,
            string definitionId,
            string revision,
            DecisionTargetRef? target) =>
            new(
                decisionKey,
                definitionId,
                revision,
                target?.Type,
                target?.Id);

        public bool Equals(StateIdentity other) =>
            string.Equals(DecisionKey, other.DecisionKey, StringComparison.Ordinal) &&
            string.Equals(DefinitionId, other.DefinitionId, StringComparison.Ordinal) &&
            string.Equals(Revision, other.Revision, StringComparison.Ordinal) &&
            string.Equals(TargetType, other.TargetType, StringComparison.Ordinal) &&
            string.Equals(TargetId, other.TargetId, StringComparison.Ordinal);

        public override bool Equals(object? obj) =>
            obj is StateIdentity other && Equals(other);

        public override int GetHashCode()
        {
            var hash = new HashCode();
            hash.Add(DecisionKey, StringComparer.Ordinal);
            hash.Add(DefinitionId, StringComparer.Ordinal);
            hash.Add(Revision, StringComparer.Ordinal);
            hash.Add(TargetType, StringComparer.Ordinal);
            hash.Add(TargetId, StringComparer.Ordinal);
            return hash.ToHashCode();
        }
    }

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
        string? LastChangedAt);

    private sealed record PersistedDecisionTarget(
        string? Type,
        string? Id);

    private sealed record PersistedNumericRule(
        string? InputSignalKey,
        JsonElement? Threshold,
        JsonElement? ValueAtOrAbove,
        JsonElement? ValueBelow,
        IReadOnlyList<PersistedNumericRuleInput?>? WeightedInputs);

    private sealed record PersistedNumericRuleInput(
        string? SignalKey,
        JsonElement? Minimum,
        JsonElement? Maximum,
        JsonElement? Weight);

    private sealed class SourceStateSnapshotProvider(
        CommittedFileSnapshotSource source) : IStateSnapshotProvider
    {
        public Task<CommittedArtifactReference> ResolveStateSnapshotAsync(
            CancellationToken cancellationToken) =>
            CommittedFileSnapshot.ResolveAsync(
                source,
                options: null,
                cancellationToken);
    }
}
