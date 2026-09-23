using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed record LocalFileGovernedStateLifecycleStoreOptions(
    string CommitDescriptorPath,
    TimeSpan? LockTimeout = null,
    TimeSpan? LockRetryDelay = null);

internal interface IGovernedStateDocumentPublisher
{
    Task PublishAsync(
        string commitDescriptorPath,
        ReadOnlyMemory<byte> bytes,
        CancellationToken cancellationToken);
}

public sealed class LocalFileGovernedStateLifecycleStore :
    IGovernedStateLifecycleStore
{
    private readonly string _commitDescriptorPath;
    private readonly string _lockPath;
    private readonly TimeSpan _lockTimeout;
    private readonly TimeSpan _lockRetryDelay;
    private readonly TimeProvider _timeProvider;
    private readonly IGovernedStateIdentityGenerator _identityGenerator;
    private readonly IGovernedStateDocumentPublisher _publisher;

    public LocalFileGovernedStateLifecycleStore(
        LocalFileGovernedStateLifecycleStoreOptions options,
        TimeProvider? timeProvider = null,
        IGovernedStateIdentityGenerator? identityGenerator = null)
        : this(
            options,
            timeProvider ?? TimeProvider.System,
            identityGenerator ?? new GuidGovernedStateIdentityGenerator(),
            new CommittedGovernedStateDocumentPublisher())
    {
    }

    internal LocalFileGovernedStateLifecycleStore(
        LocalFileGovernedStateLifecycleStoreOptions options,
        TimeProvider timeProvider,
        IGovernedStateIdentityGenerator identityGenerator,
        IGovernedStateDocumentPublisher publisher)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentException.ThrowIfNullOrWhiteSpace(
            options.CommitDescriptorPath);
        _commitDescriptorPath = Path.GetFullPath(
            options.CommitDescriptorPath);
        _lockPath = $"{_commitDescriptorPath}.lock";
        _lockTimeout = options.LockTimeout ?? TimeSpan.FromSeconds(10);
        _lockRetryDelay =
            options.LockRetryDelay ?? TimeSpan.FromMilliseconds(25);
        if (_lockTimeout <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The governed-state lock timeout must be positive.");
        }

        if (_lockRetryDelay <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The governed-state lock retry delay must be positive.");
        }

        _timeProvider = timeProvider;
        _identityGenerator = identityGenerator;
        _publisher = publisher;
    }

    public async Task<GovernedDecisionState?> GetBaselineAsync(
        GovernedStateAddress address,
        CancellationToken cancellationToken)
    {
        var store = await LoadAsync(cancellationToken);
        return await store.GetBaselineAsync(address, cancellationToken);
    }

    public Task<GovernedDecisionState> ActivateAsync(
        GovernedStateActivationRequest request,
        CancellationToken cancellationToken) =>
        MutateAsync(
            store => store.ActivateAsync(request, cancellationToken),
            cancellationToken);

    private async Task<GovernedDecisionState> MutateAsync(
        Func<
            InMemoryGovernedStateLifecycleStore,
            Task<GovernedDecisionState>> mutation,
        CancellationToken cancellationToken)
    {
        await using var lease = await AcquireLeaseAsync(cancellationToken);
        var store = await LoadAsync(cancellationToken);
        var state = await mutation(store);
        var bytes = GovernedStatePersistence.Serialize(
            store.CapturePersistenceSnapshot());
        await _publisher.PublishAsync(
            _commitDescriptorPath,
            bytes,
            cancellationToken);
        return state;
    }

    private async Task<InMemoryGovernedStateLifecycleStore> LoadAsync(
        CancellationToken cancellationToken)
    {
        GovernedStatePersistenceSnapshot snapshot;
        if (!File.Exists(_commitDescriptorPath))
        {
            snapshot = new GovernedStatePersistenceSnapshot(2, [], []);
        }
        else
        {
            var bytes = await CommittedFileSnapshot.ReadAsync(
                CommittedFileSnapshotSource.FromDescriptor(
                    _commitDescriptorPath),
                options: null,
                cancellationToken);
            snapshot = GovernedStatePersistence.Deserialize(bytes);
            if (snapshot.Version != 2)
            {
                throw new InvalidDataException(
                    "Lifecycle mutation requires a version 2 governed-state document.");
            }
        }

        return new InMemoryGovernedStateLifecycleStore(
            snapshot,
            _timeProvider,
            _identityGenerator);
    }

    private async Task<FileStream> AcquireLeaseAsync(
        CancellationToken cancellationToken)
    {
        var directory = Path.GetDirectoryName(_commitDescriptorPath)
            ?? throw new InvalidOperationException(
                "The governed-state path must include a directory.");
        Directory.CreateDirectory(directory);
        var started = Stopwatch.GetTimestamp();
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                return new FileStream(
                    _lockPath,
                    FileMode.OpenOrCreate,
                    FileAccess.ReadWrite,
                    FileShare.None,
                    1,
                    FileOptions.Asynchronous);
            }
            catch (IOException) when (
                Stopwatch.GetElapsedTime(started) < _lockTimeout)
            {
                await Task.Delay(_lockRetryDelay, cancellationToken);
            }
            catch (IOException error)
            {
                throw new TimeoutException(
                    $"Timed out acquiring the governed-state lock '{_lockPath}'.",
                    error);
            }
        }
    }

    private sealed class CommittedGovernedStateDocumentPublisher :
        IGovernedStateDocumentPublisher
    {
        public async Task PublishAsync(
            string commitDescriptorPath,
            ReadOnlyMemory<byte> bytes,
            CancellationToken cancellationToken)
        {
            await CommittedFileSnapshotWriter.PublishAsync(
                commitDescriptorPath,
                bytes,
                cancellationToken,
                artifactStem: "governed-state");
        }
    }
}

internal static partial class GovernedStatePersistence
{
    private const int CurrentVersion = 2;

    private static readonly JsonSerializerOptions ReadOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    private static readonly JsonSerializerOptions WriteOptions =
        new(JsonSerializerDefaults.Web)
        {
            WriteIndented = true
        };

    public static byte[] Serialize(GovernedStatePersistenceSnapshot snapshot)
    {
        ValidateSingleScope(snapshot.States);
        var document = new PersistedDocument(
            CurrentVersion,
            snapshot.States
                .OrderBy(entry => entry.Address.AppId, StringComparer.Ordinal)
                .ThenBy(
                    entry => entry.Address.Environment,
                    StringComparer.Ordinal)
                .ThenBy(
                    entry => entry.Address.DecisionKey,
                    StringComparer.Ordinal)
                .ThenBy(
                    entry => entry.Address.ControlTarget?.Type,
                    StringComparer.Ordinal)
                .ThenBy(
                    entry => entry.Address.ControlTarget?.Id,
                    StringComparer.Ordinal)
                .ThenBy(entry => entry.State.Generation)
                .Select(Map)
                .ToArray(),
            snapshot.Activations
                .OrderBy(replay => replay.ActivationId, StringComparer.Ordinal)
                .Select(
                replay => new PersistedActivation(
                    replay.ActivationId,
                    replay.Fingerprint,
                    replay.ProposalId,
                    replay.ProposalFingerprint,
                    replay.StateId)).ToArray());
        return JsonSerializer.SerializeToUtf8Bytes(document, WriteOptions);
    }

    public static GovernedStatePersistenceSnapshot Deserialize(
        ReadOnlyMemory<byte> bytes)
    {
        try
        {
            StrictJson.Validate(bytes.Span);
            var document = JsonSerializer.Deserialize<PersistedDocument>(
                bytes.Span,
                ReadOptions);
            if (document is null ||
                document.Version != CurrentVersion ||
                document.States is null ||
                document.Activations is null)
            {
                throw new InvalidDataException(
                    "A lifecycle governed-state document must use version 2.");
            }

            var states = document.States.Select(Map).ToArray();
            ValidateSingleScope(states);
            var activations = document.Activations
                .Select(Map)
                .ToArray();
            return new GovernedStatePersistenceSnapshot(
                CurrentVersion,
                states,
                activations);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The lifecycle governed-state document is not valid strict JSON.",
                error);
        }
    }

    private static PersistedState Map(GovernedStateEntry entry)
    {
        var state = entry.State;
        return new PersistedState(
            entry.Address.AppId,
            entry.Address.Environment,
            entry.Address.DecisionKey,
            state.DefinitionId,
            state.Revision,
            state.ContractDigest,
            state.Value.Clone(),
            state.ControlTarget,
            state.Mode,
            state.StrategyId,
            Map(state.NumericRule),
            FormatTimestamp(state.LastChangedAt),
            state.StateId,
            state.ProposalId,
            state.Generation,
            state.PredecessorStateId,
            state.ApprovalReference,
            FormatTimestamp(state.ActivatedAt),
            FormatStatus(state.LifecycleStatus));
    }

    private static GovernedStateEntry Map(PersistedState? persisted)
    {
        if (persisted is null ||
            string.IsNullOrWhiteSpace(persisted.AppId) ||
            string.IsNullOrWhiteSpace(persisted.Environment) ||
            string.IsNullOrWhiteSpace(persisted.DecisionKey) ||
            string.IsNullOrWhiteSpace(persisted.DefinitionId) ||
            string.IsNullOrWhiteSpace(persisted.Revision) ||
            !ContractDigestPattern().IsMatch(
                persisted.ContractDigest ?? string.Empty) ||
            persisted.Value is not JsonElement value ||
            string.IsNullOrWhiteSpace(persisted.Mode) ||
            string.IsNullOrWhiteSpace(persisted.StateId) ||
            string.IsNullOrWhiteSpace(persisted.ProposalId) ||
            persisted.Generation is null or <= 0 ||
            string.IsNullOrWhiteSpace(persisted.ApprovalReference))
        {
            throw new InvalidDataException(
                "A lifecycle governed-state entry is missing required identity data.");
        }

        ValidateTarget(persisted.ControlTarget);
        var numericRule = Map(persisted.NumericRule);
        ValidateValueAndMode(persisted, value, numericRule);
        var activatedAt = ParseTimestamp(
            persisted.ActivatedAt,
            "activatedAt",
            required: true);
        var lastChangedAt = ParseTimestamp(
            persisted.LastChangedAt,
            "lastChangedAt",
            required: true);
        var status = ParseStatus(persisted.LifecycleStatus);
        var state = new GovernedDecisionState(
            persisted.DefinitionId,
            persisted.Revision,
            persisted.ContractDigest!,
            value.Clone(),
            persisted.ControlTarget,
            persisted.Mode,
            persisted.StrategyId,
            numericRule,
            lastChangedAt,
            persisted.StateId,
            persisted.ProposalId,
            persisted.Generation.Value,
            persisted.PredecessorStateId,
            persisted.ApprovalReference,
            activatedAt,
            status);
        return new GovernedStateEntry(
            new GovernedStateAddress(
                persisted.AppId,
                persisted.Environment,
                persisted.DecisionKey,
                persisted.ControlTarget),
            state);
    }

    private static void ValidateSingleScope(
        IReadOnlyList<GovernedStateEntry> states)
    {
        if (states
            .Select(entry => (
                entry.Address.AppId,
                entry.Address.Environment))
            .Distinct()
            .Skip(1)
            .Any())
        {
            throw new InvalidDataException(
                "A local lifecycle governed-state document must contain exactly one application and environment scope.");
        }
    }

    private static GovernedStateActivationReplay Map(
        PersistedActivation? replay)
    {
        if (replay is null ||
            string.IsNullOrWhiteSpace(replay.ActivationId) ||
            !FingerprintPattern().IsMatch(replay.Fingerprint ?? string.Empty) ||
            string.IsNullOrWhiteSpace(replay.ProposalId) ||
            !FingerprintPattern().IsMatch(
                replay.ProposalFingerprint ?? string.Empty) ||
            string.IsNullOrWhiteSpace(replay.StateId))
        {
            throw new InvalidDataException(
                "A governed-state activation replay entry is invalid.");
        }

        return new GovernedStateActivationReplay(
            replay.ActivationId,
            replay.Fingerprint!,
            replay.ProposalId,
            replay.ProposalFingerprint!,
            replay.StateId);
    }

    private static void ValidateValueAndMode(
        PersistedState state,
        JsonElement value,
        NumericRuleStrategy? numericRule)
    {
        var validValue =
            value.ValueKind is
                JsonValueKind.True or
                JsonValueKind.False or
                JsonValueKind.String ||
            value.ValueKind == JsonValueKind.Number &&
            CanonicalJson.IsIeee754CompatibleNumber(value);
        if (!validValue)
        {
            throw new InvalidDataException(
                "A lifecycle governed-state value must be a canonical primitive.");
        }

        switch (state.Mode)
        {
            case "active-value" when
                state.StrategyId is null &&
                numericRule is null:
                return;
            case "numeric-rule" when
                !string.IsNullOrWhiteSpace(state.StrategyId) &&
                numericRule is not null &&
                value.ValueKind == JsonValueKind.Number:
                ValidateNumericRule(numericRule);
                return;
            case "active-value":
            case "numeric-rule":
                throw new InvalidDataException(
                    "A lifecycle governed-state entry has incoherent mode data.");
            default:
                throw new InvalidDataException(
                    $"Lifecycle governed-state mode '{state.Mode}' is unsupported.");
        }
    }

    private static PersistedNumericRule? Map(
        NumericRuleStrategy? rule) =>
        rule is null
            ? null
            : new PersistedNumericRule(
                rule.InputSignalKey,
                JsonSerializer.SerializeToElement(rule.Threshold),
                JsonSerializer.SerializeToElement(rule.ValueAtOrAbove),
                JsonSerializer.SerializeToElement(rule.ValueBelow),
                rule.WeightedInputs?.Select(input =>
                    new PersistedNumericRuleInput(
                        input.SignalKey,
                        JsonSerializer.SerializeToElement(input.Minimum),
                        JsonSerializer.SerializeToElement(input.Maximum),
                        JsonSerializer.SerializeToElement(input.Weight)))
                    .ToArray());

    private static NumericRuleStrategy? Map(
        PersistedNumericRule? rule)
    {
        if (rule is null)
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(rule.InputSignalKey) ||
            !TryGetCanonicalDouble(rule.Threshold, out var threshold) ||
            !TryGetCanonicalDouble(
                rule.ValueAtOrAbove,
                out var valueAtOrAbove) ||
            !TryGetCanonicalDouble(rule.ValueBelow, out var valueBelow))
        {
            throw new InvalidDataException(
                "A lifecycle numeric rule contains invalid scalar configuration.");
        }

        if (rule.WeightedInputs is null)
        {
            return new NumericRuleStrategy(
                rule.InputSignalKey,
                threshold,
                valueAtOrAbove,
                valueBelow,
                null);
        }

        if (rule.WeightedInputs.Count == 0 ||
            threshold is < 0 or > 1)
        {
            throw new InvalidDataException(
                "A weighted lifecycle numeric rule requires inputs and a threshold from zero through one.");
        }

        var inputs = rule.WeightedInputs;
        var mappedInputs = new List<NumericRuleInput>(inputs.Count);
        foreach (var input in inputs)
        {
            if (input is null ||
                !TryGetCanonicalDouble(input.Minimum, out var minimum) ||
                !TryGetCanonicalDouble(input.Maximum, out var maximum) ||
                !TryGetCanonicalDouble(input.Weight, out var weight))
            {
                throw new InvalidDataException(
                    "A lifecycle numeric rule contains invalid weighted input configuration.");
            }

            mappedInputs.Add(
                new NumericRuleInput(
                    input.SignalKey ?? string.Empty,
                    minimum,
                    maximum,
                    weight));
        }

        return new NumericRuleStrategy(
            rule.InputSignalKey,
            threshold,
            valueAtOrAbove,
            valueBelow,
            mappedInputs);
    }

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

    private static void ValidateNumericRule(NumericRuleStrategy rule)
    {
        if (string.IsNullOrWhiteSpace(rule.InputSignalKey) ||
            !double.IsFinite(rule.Threshold) ||
            !double.IsFinite(rule.ValueAtOrAbove) ||
            !double.IsFinite(rule.ValueBelow))
        {
            throw new InvalidDataException(
                "A lifecycle numeric rule contains invalid scalar configuration.");
        }

        if (rule.WeightedInputs is null)
        {
            return;
        }

        if (rule.WeightedInputs.Count == 0 ||
            rule.Threshold is < 0 or > 1)
        {
            throw new InvalidDataException(
                "A weighted lifecycle numeric rule requires inputs and a threshold from zero through one.");
        }

        var inputs = rule.WeightedInputs;
        var keys = new HashSet<string>(StringComparer.Ordinal);
        var totalWeight = 0d;
        foreach (var input in inputs)
        {
            if (string.IsNullOrWhiteSpace(input.SignalKey) ||
                !keys.Add(input.SignalKey) ||
                !double.IsFinite(input.Minimum) ||
                !double.IsFinite(input.Maximum) ||
                input.Maximum <= input.Minimum ||
                !double.IsFinite(input.Weight) ||
                input.Weight < 0)
            {
                throw new InvalidDataException(
                    "A lifecycle numeric rule contains an invalid weighted input.");
            }

            totalWeight += input.Weight;
        }

        if (!double.IsFinite(totalWeight) || totalWeight <= 0)
        {
            throw new InvalidDataException(
                "A lifecycle numeric rule must have positive total weight.");
        }
    }

    private static void ValidateTarget(DecisionTargetRef? target)
    {
        if (target is not null &&
            (string.IsNullOrWhiteSpace(target.Type) ||
             string.IsNullOrWhiteSpace(target.Id)))
        {
            throw new InvalidDataException(
                "A lifecycle governed-state target requires type and id.");
        }
    }

    private static string? FormatTimestamp(DateTimeOffset? value) =>
        value?.ToUniversalTime().ToString("O");

    private static DateTimeOffset? ParseTimestamp(
        string? value,
        string property,
        bool required)
    {
        if (value is null)
        {
            if (required)
            {
                throw new InvalidDataException(
                    $"A lifecycle governed-state {property} value is required.");
            }

            return null;
        }

        if (!DateTimeOffset.TryParseExact(
                value,
                "O",
                System.Globalization.CultureInfo.InvariantCulture,
                System.Globalization.DateTimeStyles.None,
                out var timestamp) ||
            timestamp == default)
        {
            throw new InvalidDataException(
                $"A lifecycle governed-state {property} value is invalid.");
        }

        return timestamp.ToUniversalTime();
    }

    private static string FormatStatus(GovernedDecisionStateStatus status) =>
        status switch
        {
            GovernedDecisionStateStatus.Active => "active",
            GovernedDecisionStateStatus.Superseded => "superseded",
            _ => throw new InvalidDataException(
                "The governed-state lifecycle status is unsupported.")
        };

    private static GovernedDecisionStateStatus ParseStatus(string? status) =>
        status switch
        {
            "active" => GovernedDecisionStateStatus.Active,
            "superseded" => GovernedDecisionStateStatus.Superseded,
            _ => throw new InvalidDataException(
                "A lifecycle governed-state status is invalid.")
        };

    [GeneratedRegex(
        "\\Asha256:[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex ContractDigestPattern();

    [GeneratedRegex(
        "\\A[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex FingerprintPattern();

    private sealed record PersistedDocument(
        int? Version,
        IReadOnlyList<PersistedState?>? States,
        IReadOnlyList<PersistedActivation?>? Activations);

    private sealed record PersistedState(
        string? AppId,
        string? Environment,
        string? DecisionKey,
        string? DefinitionId,
        string? Revision,
        string? ContractDigest,
        JsonElement? Value,
        DecisionTargetRef? ControlTarget,
        string? Mode,
        string? StrategyId,
        PersistedNumericRule? NumericRule,
        string? LastChangedAt,
        string? StateId,
        string? ProposalId,
        long? Generation,
        string? PredecessorStateId,
        string? ApprovalReference,
        string? ActivatedAt,
        string? LifecycleStatus);

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

    private sealed record PersistedActivation(
        string? ActivationId,
        string? Fingerprint,
        string? ProposalId,
        string? ProposalFingerprint,
        string? StateId);
}
