using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

[JsonConverter(typeof(JsonStringEnumConverter<GovernedDecisionStateStatus>))]
public enum GovernedDecisionStateStatus
{
    Active,
    Superseded
}

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

[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(
    typeof(ActiveValueActivationCandidate),
    "active-value")]
[JsonDerivedType(
    typeof(NumericRuleActivationCandidate),
    "numeric-rule")]
public abstract record GovernedStateActivationCandidate(string Rationale);

public sealed record ActiveValueActivationCandidate(
    string Rationale,
    JsonElement Value) : GovernedStateActivationCandidate(Rationale);

public sealed record NumericRuleActivationCandidate(
    string Rationale,
    JsonElement InitialValue,
    NumericRuleStrategy Rule) : GovernedStateActivationCandidate(Rationale);

public sealed record GovernedStateActivationRequest(
    string ActivationId,
    string ProposalId,
    string ApprovalReference,
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef? ControlTarget,
    GovernedStateBaseline ExpectedBaseline,
    GovernedStateActivationCandidate Candidate);

public interface IGovernedStateIdentityGenerator
{
    string CreateStateId();
}

public sealed class GuidGovernedStateIdentityGenerator :
    IGovernedStateIdentityGenerator
{
    public string CreateStateId() => Guid.NewGuid().ToString("N");
}

public interface IGovernedStateLifecycleStore
{
    Task<GovernedDecisionState?> GetBaselineAsync(
        GovernedStateAddress address,
        CancellationToken cancellationToken);

    Task<GovernedDecisionState> ActivateAsync(
        GovernedStateActivationRequest request,
        CancellationToken cancellationToken);
}

public sealed class GovernedStateConflictException(
    string code,
    string message) : Exception(message)
{
    public string Code { get; } = code;
}

public sealed class GovernedStateValidationException(
    string code,
    string message) : Exception(message)
{
    public string Code { get; } = code;
}

public sealed class GovernedStateRuntimeProjection(
    IGovernedStateLifecycleStore lifecycleStore,
    string appId,
    string environment) : IStateStore
{
    public async Task<GovernedDecisionState?> GetActiveAsync(
        string decisionKey,
        string definitionId,
        string revision,
        IReadOnlyList<DecisionTargetRef?> resolutionTargets,
        CancellationToken cancellationToken)
    {
        foreach (var target in resolutionTargets)
        {
            var state = await lifecycleStore.GetBaselineAsync(
                new GovernedStateAddress(
                    appId,
                    environment,
                    decisionKey,
                    target),
                cancellationToken);
            if (state is not null &&
                state.LifecycleStatus == GovernedDecisionStateStatus.Active &&
                string.Equals(
                    state.DefinitionId,
                    definitionId,
                    StringComparison.Ordinal) &&
                string.Equals(
                    state.Revision,
                    revision,
                    StringComparison.Ordinal))
            {
                return state;
            }
        }

        return null;
    }
}

public sealed partial class InMemoryGovernedStateLifecycleStore :
    IGovernedStateLifecycleStore
{
    private static readonly JsonSerializerOptions FingerprintOptions =
        new(JsonSerializerDefaults.Web);

    private readonly object _gate = new();
    private readonly Dictionary<GovernedStateAddress, string> _latestStateIds = [];
    private readonly Dictionary<string, GovernedStateEntry> _states = [];
    private readonly Dictionary<string, GovernedStateActivationReplay> _activations = [];
    private readonly Dictionary<string, string> _proposalFingerprints = [];
    private readonly TimeProvider _timeProvider;
    private readonly IGovernedStateIdentityGenerator _identityGenerator;

    public InMemoryGovernedStateLifecycleStore(
        TimeProvider? timeProvider = null,
        IGovernedStateIdentityGenerator? identityGenerator = null)
        : this(
            new GovernedStatePersistenceSnapshot(2, [], []),
            timeProvider ?? TimeProvider.System,
            identityGenerator ?? new GuidGovernedStateIdentityGenerator())
    {
    }

    internal InMemoryGovernedStateLifecycleStore(
        GovernedStatePersistenceSnapshot snapshot,
        TimeProvider timeProvider,
        IGovernedStateIdentityGenerator identityGenerator)
    {
        _timeProvider = timeProvider;
        _identityGenerator = identityGenerator;
        var activeAddresses = new HashSet<GovernedStateAddress>();

        foreach (var entry in snapshot.States)
        {
            if (string.IsNullOrWhiteSpace(entry.State.StateId) ||
                !_states.TryAdd(entry.State.StateId, entry))
            {
                throw new InvalidDataException(
                    "Governed lifecycle state identities must be nonempty and unique.");
            }

            if (entry.State.LifecycleStatus == GovernedDecisionStateStatus.Active &&
                !activeAddresses.Add(entry.Address))
            {
                throw new InvalidDataException(
                    "Only one governed state may be active for an authority address.");
            }

            if (!_latestStateIds.TryGetValue(entry.Address, out var latestStateId) ||
                _states[latestStateId].State.Generation < entry.State.Generation)
            {
                _latestStateIds[entry.Address] = entry.State.StateId;
            }
            else if (_states[latestStateId].State.Generation ==
                     entry.State.Generation)
            {
                throw new InvalidDataException(
                    "Governed state generations must be unique within an authority address.");
            }
        }

        foreach (var (address, latestStateId) in _latestStateIds)
        {
            var latestState = _states[latestStateId].State;
            if (latestState.LifecycleStatus != GovernedDecisionStateStatus.Active)
            {
                throw new InvalidDataException(
                    "The latest governed state for an authority address must be active.");
            }

            if (_states.Values.Any(entry =>
                    entry.Address == address &&
                    !string.Equals(
                        entry.State.StateId,
                        latestStateId,
                        StringComparison.Ordinal) &&
                    entry.State.LifecycleStatus !=
                        GovernedDecisionStateStatus.Superseded))
            {
                throw new InvalidDataException(
                    "Every predecessor governed state must be superseded.");
            }
        }

        foreach (var addressStates in snapshot.States.GroupBy(
                     entry => entry.Address))
        {
            var ordered = addressStates
                .OrderBy(entry => entry.State.Generation)
                .ToArray();
            if (ordered[0].State.Generation != 1 ||
                ordered[0].State.PredecessorStateId is not null)
            {
                throw new InvalidDataException(
                    "Governed state lineage must begin at generation one without a predecessor.");
            }

            for (var index = 1; index < ordered.Length; index++)
            {
                var predecessor = ordered[index - 1].State;
                var current = ordered[index].State;
                if (current.Generation != predecessor.Generation + 1 ||
                    !string.Equals(
                        current.PredecessorStateId,
                        predecessor.StateId,
                        StringComparison.Ordinal))
                {
                    throw new InvalidDataException(
                        "Governed state lineage must be contiguous within one authority address.");
                }
            }
        }

        var referencedStateIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var replay in snapshot.Activations)
        {
            if (!_states.TryGetValue(replay.StateId, out var entry) ||
                !referencedStateIds.Add(replay.StateId) ||
                !_activations.TryAdd(replay.ActivationId, replay) ||
                !_proposalFingerprints.TryAdd(
                    replay.ProposalId,
                    replay.ProposalFingerprint) ||
                !string.Equals(
                    replay.ProposalId,
                    entry.State.ProposalId,
                    StringComparison.Ordinal))
            {
                throw new InvalidDataException(
                    "Governed activation identities must uniquely bind to their stored state.");
            }

            if (entry.State.Mode == "numeric-rule" &&
                !string.Equals(
                    entry.State.StrategyId,
                    CreateStrategyId(
                        replay.ActivationId,
                        entry.State.NumericRule!),
                    StringComparison.Ordinal))
            {
                throw new InvalidDataException(
                    "A governed numeric-rule strategy identity must match its activation.");
            }
        }

        if (referencedStateIds.Count != _states.Count)
        {
            throw new InvalidDataException(
                "Every governed state must have exactly one activation replay.");
        }
    }

    public Task<GovernedDecisionState?> GetBaselineAsync(
        GovernedStateAddress address,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateAddress(address);
        lock (_gate)
        {
            return Task.FromResult(
                _latestStateIds.TryGetValue(address, out var stateId)
                    ? _states[stateId].State
                    : null);
        }
    }

    public Task<GovernedDecisionState> ActivateAsync(
        GovernedStateActivationRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateActivation(request);
        var requestFingerprint = Fingerprint(request);
        var proposalFingerprint = Fingerprint(
            new GovernedStateProposalFingerprint(
                request.ProposalId,
                request.Definition,
                request.ControlTarget,
                request.ExpectedBaseline,
                request.Candidate));

        lock (_gate)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (_activations.TryGetValue(request.ActivationId, out var replay))
            {
                if (!string.Equals(
                        replay.Fingerprint,
                        requestFingerprint,
                        StringComparison.Ordinal))
                {
                    throw Conflict(
                        "activation-conflict",
                        "The activation identity was reused with different content.");
                }

                return Task.FromResult(_states[replay.StateId].State);
            }

            if (_proposalFingerprints.TryGetValue(
                    request.ProposalId,
                    out var priorProposalFingerprint))
            {
                throw Conflict(
                    "duplicate-proposal",
                    string.Equals(
                        priorProposalFingerprint,
                        proposalFingerprint,
                        StringComparison.Ordinal)
                        ? "The proposal was already activated under another activation identity."
                        : "The proposal identity was reused with different content.");
            }

            var address = Address(request);
            var current = ResolveExpectedBaseline(
                address,
                request.ExpectedBaseline);
            ValidateDefinitionCompatibility(current, request.Definition);

            var stateId = _identityGenerator.CreateStateId();
            if (string.IsNullOrWhiteSpace(stateId) ||
                _states.ContainsKey(stateId))
            {
                throw new InvalidOperationException(
                    "The governed state identity generator returned an invalid or duplicate identity.");
            }

            var activatedAt = _timeProvider.GetUtcNow().ToUniversalTime();
            var generation = checked((current?.Generation ?? 0) + 1);
            var state = CreateState(
                request,
                stateId,
                generation,
                current?.StateId,
                activatedAt);

            if (current is not null)
            {
                _states[current.StateId!] = new GovernedStateEntry(
                    address,
                    current with
                    {
                        LifecycleStatus =
                            GovernedDecisionStateStatus.Superseded
                    });
            }

            _states.Add(stateId, new GovernedStateEntry(address, state));
            _latestStateIds[address] = stateId;
            _proposalFingerprints.Add(
                request.ProposalId,
                proposalFingerprint);
            _activations.Add(
                request.ActivationId,
                new GovernedStateActivationReplay(
                    request.ActivationId,
                    requestFingerprint,
                    request.ProposalId,
                    proposalFingerprint,
                    stateId));
            return Task.FromResult(state);
        }
    }

    internal GovernedStatePersistenceSnapshot CapturePersistenceSnapshot()
    {
        lock (_gate)
        {
            return new GovernedStatePersistenceSnapshot(
                2,
                _states.Values.ToArray(),
                _activations.Values.ToArray());
        }
    }

    private GovernedDecisionState? ResolveExpectedBaseline(
        GovernedStateAddress address,
        GovernedStateBaseline expected)
    {
        _latestStateIds.TryGetValue(address, out var latestStateId);
        if (expected.StateId is null)
        {
            if (latestStateId is not null)
            {
                throw Conflict(
                    "stale-baseline",
                    "A governed state already exists for the authority address.");
            }

            return null;
        }

        if (!_states.TryGetValue(expected.StateId, out var expectedEntry))
        {
            throw Conflict(
                "stale-baseline",
                "The expected governed state does not exist.");
        }

        if (expectedEntry.Address != address)
        {
            throw Conflict(
                "target-conflict",
                "The expected governed state belongs to another authority address.");
        }

        if (!string.Equals(
                latestStateId,
                expected.StateId,
                StringComparison.Ordinal) ||
            expectedEntry.State.Generation != expected.Generation)
        {
            throw Conflict(
                "stale-baseline",
                "The expected governed state is no longer the latest generation.");
        }

        return expectedEntry.State;
    }

    private static void ValidateDefinitionCompatibility(
        GovernedDecisionState? current,
        GovernedDefinitionIdentity definition)
    {
        if (current is null)
        {
            return;
        }

        var contract = definition.Contract;
        if (string.Equals(
                current.DefinitionId,
                contract.DefinitionId,
                StringComparison.Ordinal) &&
            string.Equals(
                current.Revision,
                contract.Revision,
                StringComparison.Ordinal) &&
            !string.Equals(
                current.ContractDigest,
                contract.ContractDigest,
                StringComparison.Ordinal))
        {
            throw Conflict(
                "incompatible-definition",
                "The activation changed the digest of an existing definition revision.");
        }
    }

    private static GovernedDecisionState CreateState(
        GovernedStateActivationRequest request,
        string stateId,
        long generation,
        string? predecessorStateId,
        DateTimeOffset activatedAt)
    {
        var contract = request.Definition.Contract;
        return request.Candidate switch
        {
            ActiveValueActivationCandidate activeValue =>
                new GovernedDecisionState(
                    contract.DefinitionId,
                    contract.Revision,
                    contract.ContractDigest,
                    activeValue.Value.Clone(),
                    request.ControlTarget,
                    LastChangedAt: activatedAt,
                    StateId: stateId,
                    ProposalId: request.ProposalId,
                    Generation: generation,
                    PredecessorStateId: predecessorStateId,
                    ApprovalReference: request.ApprovalReference,
                    ActivatedAt: activatedAt),
            NumericRuleActivationCandidate numericRule =>
                new GovernedDecisionState(
                    contract.DefinitionId,
                    contract.Revision,
                    contract.ContractDigest,
                    numericRule.InitialValue.Clone(),
                    request.ControlTarget,
                    "numeric-rule",
                    CreateStrategyId(
                        request.ActivationId,
                        numericRule.Rule),
                    CloneStrategy(numericRule.Rule),
                    activatedAt,
                    stateId,
                    request.ProposalId,
                    generation,
                    predecessorStateId,
                    request.ApprovalReference,
                    activatedAt),
            _ => throw Validation(
                "unsupported-state-kind",
                "The activation candidate kind is not supported by this state adapter.")
        };
    }

    private static string CreateStrategyId(
        string activationId,
        NumericRuleStrategy rule) =>
        $"strategy_{Fingerprint(new GovernedStateStrategyIdentity(
            activationId,
            rule))}";

    private static void ValidateActivation(
        GovernedStateActivationRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        if (string.IsNullOrWhiteSpace(request.ActivationId) ||
            string.IsNullOrWhiteSpace(request.ProposalId) ||
            string.IsNullOrWhiteSpace(request.ApprovalReference))
        {
            throw Validation(
                "invalid-activation",
                "Activation, proposal, and approval identities are required.");
        }

        ValidateDefinition(request.Definition);
        ValidateTarget(request.ControlTarget);
        ValidateBaseline(request.ExpectedBaseline);
        ValidateCandidate(request.Candidate);
    }

    private static void ValidateCandidate(
        GovernedStateActivationCandidate candidate)
    {
        if (candidate is null ||
            string.IsNullOrWhiteSpace(candidate.Rationale))
        {
            throw Validation(
                "invalid-activation",
                "An activation candidate and rationale are required.");
        }

        switch (candidate)
        {
            case ActiveValueActivationCandidate activeValue:
                ValidateDecisionValue(activeValue.Value);
                break;
            case NumericRuleActivationCandidate numericRule:
                ValidateDecisionValue(numericRule.InitialValue);
                if (numericRule.InitialValue.ValueKind != JsonValueKind.Number ||
                    numericRule.Rule is null)
                {
                    throw Validation(
                        "invalid-activation",
                        "A numeric-rule activation requires a numeric initial value and rule.");
                }

                ValidateNumericStrategy(numericRule.Rule);
                break;
            default:
                throw Validation(
                    "unsupported-state-kind",
                    "The activation candidate kind is not supported by this state adapter.");
        }
    }

    private static void ValidateDefinition(
        GovernedDefinitionIdentity definition)
    {
        if (definition is null ||
            definition.Contract is null ||
            string.IsNullOrWhiteSpace(definition.AppId) ||
            string.IsNullOrWhiteSpace(definition.Environment) ||
            string.IsNullOrWhiteSpace(definition.DecisionKey) ||
            string.IsNullOrWhiteSpace(definition.Contract.DefinitionId) ||
            string.IsNullOrWhiteSpace(definition.Contract.Revision) ||
            !ContractDigestPattern().IsMatch(
                definition.Contract.ContractDigest ?? string.Empty))
        {
            throw Validation(
                "invalid-activation",
                "The activation must carry a complete canonical definition identity.");
        }
    }

    private static void ValidateAddress(GovernedStateAddress address)
    {
        ArgumentNullException.ThrowIfNull(address);
        if (string.IsNullOrWhiteSpace(address.AppId) ||
            string.IsNullOrWhiteSpace(address.Environment) ||
            string.IsNullOrWhiteSpace(address.DecisionKey))
        {
            throw Validation(
                "invalid-state-address",
                "A governed state address requires application, environment, and decision key.");
        }

        ValidateTarget(address.ControlTarget);
    }

    private static void ValidateTarget(DecisionTargetRef? target)
    {
        if (target is not null &&
            (string.IsNullOrWhiteSpace(target.Type) ||
             string.IsNullOrWhiteSpace(target.Id)))
        {
            throw Validation(
                "invalid-state-address",
                "A governed state target requires type and id.");
        }
    }

    private static void ValidateBaseline(GovernedStateBaseline baseline)
    {
        ArgumentNullException.ThrowIfNull(baseline);
        if (baseline.StateId is null && baseline.Generation != 0 ||
            baseline.StateId is not null &&
            (string.IsNullOrWhiteSpace(baseline.StateId) ||
             baseline.Generation <= 0))
        {
            throw Validation(
                "invalid-activation",
                "The expected baseline must be empty at generation zero or identify a positive generation.");
        }
    }

    private static void ValidateDecisionValue(JsonElement value)
    {
        if (value.ValueKind is
            JsonValueKind.True or
            JsonValueKind.False or
            JsonValueKind.String)
        {
            return;
        }

        if (value.ValueKind == JsonValueKind.Number &&
            CanonicalJson.IsIeee754CompatibleNumber(value))
        {
            return;
        }

        throw Validation(
            "invalid-activation",
            "A governed value must be a canonical primitive decision value.");
    }

    private static void ValidateNumericStrategy(NumericRuleStrategy strategy)
    {
        ArgumentNullException.ThrowIfNull(strategy);
        if (string.IsNullOrWhiteSpace(strategy.InputSignalKey) ||
            !double.IsFinite(strategy.Threshold) ||
            !double.IsFinite(strategy.ValueAtOrAbove) ||
            !double.IsFinite(strategy.ValueBelow))
        {
            throw Validation(
                "invalid-activation",
                "A numeric rule contains invalid scalar configuration.");
        }

        if (strategy.WeightedInputs is null)
        {
            return;
        }

        if (strategy.WeightedInputs.Count == 0 ||
            strategy.Threshold is < 0 or > 1)
        {
            throw Validation(
                "invalid-activation",
                "A weighted numeric rule requires inputs and a threshold from zero through one.");
        }

        var weightedInputs = strategy.WeightedInputs;
        var keys = new HashSet<string>(StringComparer.Ordinal);
        var totalWeight = 0d;
        foreach (var input in weightedInputs)
        {
            if (string.IsNullOrWhiteSpace(input.SignalKey) ||
                !keys.Add(input.SignalKey) ||
                !double.IsFinite(input.Minimum) ||
                !double.IsFinite(input.Maximum) ||
                input.Maximum <= input.Minimum ||
                !double.IsFinite(input.Weight) ||
                input.Weight < 0)
            {
                throw Validation(
                    "invalid-activation",
                    "A numeric rule contains an invalid weighted input.");
            }

            totalWeight += input.Weight;
        }

        if (!double.IsFinite(totalWeight) || totalWeight <= 0)
        {
            throw Validation(
                "invalid-activation",
                "A numeric rule must have positive total weight.");
        }
    }

    private static NumericRuleStrategy CloneStrategy(
        NumericRuleStrategy strategy) =>
        strategy with
        {
            WeightedInputs = strategy.WeightedInputs?.ToArray()
        };

    private static GovernedStateAddress Address(
        GovernedStateActivationRequest request) =>
        new(
            request.Definition.AppId,
            request.Definition.Environment,
            request.Definition.DecisionKey,
            request.ControlTarget);

    private static string Fingerprint<T>(T value)
    {
        var element = JsonSerializer.SerializeToElement(
            value,
            FingerprintOptions);
        var bytes = CanonicalJson.Canonicalize(element);
        return Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    }

    private static GovernedStateConflictException Conflict(
        string code,
        string message) =>
        new(code, message);

    private static GovernedStateValidationException Validation(
        string code,
        string message) =>
        new(code, message);

    [GeneratedRegex(
        "\\Asha256:[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex ContractDigestPattern();
}

internal sealed record GovernedStateEntry(
    GovernedStateAddress Address,
    GovernedDecisionState State);

internal sealed record GovernedStateActivationReplay(
    string ActivationId,
    string Fingerprint,
    string ProposalId,
    string ProposalFingerprint,
    string StateId);

internal sealed record GovernedStatePersistenceSnapshot(
    int Version,
    IReadOnlyList<GovernedStateEntry> States,
    IReadOnlyList<GovernedStateActivationReplay> Activations);

internal sealed record GovernedStateProposalFingerprint(
    string ProposalId,
    GovernedDefinitionIdentity Definition,
    DecisionTargetRef? ControlTarget,
    GovernedStateBaseline ExpectedBaseline,
    GovernedStateActivationCandidate Candidate);

internal sealed record GovernedStateStrategyIdentity(
    string ActivationId,
    NumericRuleStrategy Rule);
