using System.Security.Cryptography;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

internal sealed record GovernedStateActivationRequest(
    string ActivationId,
    string ApprovalReference,
    DecisionProposal Proposal,
    GovernedDecisionStateStatus ReplacedStateStatus =
        GovernedDecisionStateStatus.Superseded);

internal sealed record GovernedStateTransitionRequest(
    string TransitionId,
    GovernedStateAddress Address,
    string StateId,
    long ExpectedGeneration,
    GovernedDecisionStateStatus Status);

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

    Task<GovernedDecisionState?> GetStateAsync(
        string stateId,
        CancellationToken cancellationToken);

    // Snapshot observations do not acknowledge durability; use replay or commit for that.
    Task<LifecycleReviewRecord?> GetReviewAsync(
        string reviewId,
        CancellationToken cancellationToken);

    Task<LifecycleActivationReceipt?> GetActivationAsync(
        string activationId,
        CancellationToken cancellationToken);

    Task<LifecycleTransitionReceipt?> GetTransitionAsync(
        string transitionId,
        CancellationToken cancellationToken);

    Task<LifecycleReviewReceipt> ReplayReviewAsync(
        LifecycleReviewRequest request,
        LifecycleActor actor,
        CancellationToken cancellationToken);

    Task<LifecycleActivationReceipt> ReplayActivationAsync(
        LifecycleActivationRequest request,
        LifecycleActor actor,
        CancellationToken cancellationToken);

    Task<LifecycleReviewReceipt> CommitReviewAsync(
        LifecycleReviewCommit commit,
        CancellationToken cancellationToken);

    Task<LifecycleActivationReceipt> CommitActivationAsync(
        LifecycleActivationCommit commit,
        CancellationToken cancellationToken);

    Task<LifecycleTransitionReceipt> CommitTransitionAsync(
        LifecycleTransitionCommit commit,
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
    IGovernedStateLifecycleStore, ILifecycleAuditReader
{
    private static readonly JsonSerializerOptions FingerprintOptions =
        new(JsonSerializerDefaults.Web);

    private readonly object _gate = new();
    private Dictionary<GovernedStateAddress, string> _latestStateIds = [];
    private Dictionary<string, GovernedStateEntry> _states = [];
    private Dictionary<string, GovernedStateActivationReplay> _activations = [];
    private Dictionary<string, GovernedStateTransitionReplay> _transitions = [];
    private Dictionary<string, string> _proposalFingerprints = [];
    private LifecycleAuditTrail _journal;
    private readonly TimeProvider _timeProvider;
    private readonly IGovernedStateIdentityGenerator _identityGenerator;

    public InMemoryGovernedStateLifecycleStore(
        TimeProvider? timeProvider = null,
        IGovernedStateIdentityGenerator? identityGenerator = null)
        : this(
            new GovernedStatePersistenceSnapshot(3, [], [], [], new([], [], [], [])),
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
        _journal = snapshot.Journal;
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

        foreach (var replay in snapshot.Activations)
        {
            if (!_activations.TryAdd(replay.ActivationId, replay) ||
                !_proposalFingerprints.TryAdd(
                    replay.ProposalId,
                    replay.ProposalFingerprint))
            {
                throw new InvalidDataException(
                    "Governed activation identities must be unique.");
            }
        }

        foreach (var replay in snapshot.Transitions)
        {
            if (!_transitions.TryAdd(replay.TransitionId, replay))
            {
                throw new InvalidDataException(
                    "Governed transition identities must be unique.");
            }
        }

        foreach (var address in activeAddresses)
        {
            var latestState = _states[_latestStateIds[address]].State;
            if (latestState.LifecycleStatus !=
                GovernedDecisionStateStatus.Active)
            {
                throw new InvalidDataException(
                    "An older governed state cannot remain active after a later generation.");
            }
        }

        ValidateJournalState();
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
                    ? LifecycleJson.Copy(_states[stateId].State)
                    : null);
        }
    }

    public Task<GovernedDecisionState?> GetStateAsync(
        string stateId,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ArgumentException.ThrowIfNullOrWhiteSpace(stateId);
        lock (_gate)
        {
            return Task.FromResult(
                _states.TryGetValue(stateId, out var entry)
                    ? LifecycleJson.Copy(entry.State)
                    : null);
        }
    }

    private GovernedDecisionState Activate(
        GovernedStateActivationRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateActivation(request);
        var requestFingerprint = Fingerprint(request);
        var proposalFingerprint = Fingerprint(request.Proposal);

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

                return _states[replay.StateId].State;
            }

            var proposalId = request.Proposal.Context.ProposalId;
            if (_proposalFingerprints.TryGetValue(
                    proposalId,
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

            var address = Address(request.Proposal.Context);
            var current = ResolveExpectedBaseline(
                address,
                request.Proposal.Context.ExpectedBaseline);
            ValidateDefinitionCompatibility(current, request.Proposal.Context);

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

            if (current?.LifecycleStatus == GovernedDecisionStateStatus.Active)
            {
                var replaced = current with
                {
                    LifecycleStatus = request.ReplacedStateStatus
                };
                _states[current.StateId!] =
                    new GovernedStateEntry(address, replaced);
            }

            _states.Add(stateId, new GovernedStateEntry(address, state));
            _latestStateIds[address] = stateId;
            _proposalFingerprints.Add(proposalId, proposalFingerprint);
            _activations.Add(
                request.ActivationId,
                new GovernedStateActivationReplay(
                    request.ActivationId,
                    requestFingerprint,
                    proposalId,
                    proposalFingerprint,
                    stateId));
            return state;
        }
    }

    private GovernedDecisionState Transition(
        GovernedStateTransitionRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateTransition(request);
        var fingerprint = Fingerprint(request);

        lock (_gate)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (_transitions.TryGetValue(request.TransitionId, out var replay))
            {
                if (!string.Equals(
                        replay.Fingerprint,
                        fingerprint,
                        StringComparison.Ordinal))
                {
                    throw Conflict(
                        "transition-conflict",
                        "The transition identity was reused with different content.");
                }

                return _states[replay.StateId].State;
            }

            if (!_states.TryGetValue(request.StateId, out var entry))
            {
                throw Conflict(
                    "stale-baseline",
                    "The governed state baseline no longer exists.");
            }

            if (entry.Address != request.Address)
            {
                throw Conflict(
                    "target-conflict",
                    "The governed state belongs to another authority address.");
            }

            if (!_latestStateIds.TryGetValue(
                    request.Address,
                    out var latestStateId) ||
                !string.Equals(
                    latestStateId,
                    request.StateId,
                    StringComparison.Ordinal) ||
                entry.State.Generation != request.ExpectedGeneration)
            {
                throw Conflict(
                    "stale-baseline",
                    "The governed state baseline was replaced before the transition.");
            }

            if (entry.State.LifecycleStatus !=
                GovernedDecisionStateStatus.Active)
            {
                throw Validation(
                    "invalid-lifecycle-transition",
                    "Only an active governed state may be completed or expired.");
            }

            var transitioned = entry.State with
            {
                LifecycleStatus = request.Status
            };
            _states[request.StateId] =
                new GovernedStateEntry(request.Address, transitioned);
            _transitions.Add(
                request.TransitionId,
                new GovernedStateTransitionReplay(
                    request.TransitionId,
                    fingerprint,
                    request.StateId));
            return transitioned;
        }
    }

    internal GovernedStatePersistenceSnapshot CapturePersistenceSnapshot()
    {
        lock (_gate)
        {
            return new GovernedStatePersistenceSnapshot(
                3,
                _states.Values.ToArray(),
                _activations.Values.ToArray(),
                _transitions.Values.ToArray(),
                _journal);
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
        DecisionProposalContext context)
    {
        if (current is null)
        {
            return;
        }

        var contract = context.Definition.Contract;
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
                "The proposal changed the digest of an existing definition revision.");
        }
    }

    private static GovernedDecisionState CreateState(
        GovernedStateActivationRequest request,
        string stateId,
        long generation,
        string? predecessorStateId,
        DateTimeOffset activatedAt)
    {
        var context = request.Proposal.Context;
        var contract = context.Definition.Contract;
        return request.Proposal switch
        {
            FixedValueDecisionProposal fixedValue =>
                new GovernedDecisionState(
                    contract.DefinitionId,
                    contract.Revision,
                    contract.ContractDigest,
                    fixedValue.Value.Clone(),
                    context.ControlTarget,
                    LastChangedAt: activatedAt,
                    StateId: stateId,
                    ProposalId: context.ProposalId,
                    Generation: generation,
                    PredecessorStateId: predecessorStateId,
                    ApprovalReference: request.ApprovalReference,
                    ActivatedAt: activatedAt),
            NumericStrategyDecisionProposal strategy =>
                new GovernedDecisionState(
                    contract.DefinitionId,
                    contract.Revision,
                    contract.ContractDigest,
                    strategy.InitialValue.Clone(),
                    context.ControlTarget,
                    "strategy",
                    strategy.StrategyId,
                    CloneStrategy(strategy.Strategy),
                    activatedAt,
                    stateId,
                    context.ProposalId,
                    generation,
                    predecessorStateId,
                    request.ApprovalReference,
                    activatedAt),
            _ => throw Validation(
                "unsupported-state-kind",
                "The decision proposal kind is not supported by this state adapter.")
        };
    }

    private void ValidateActivation(GovernedStateActivationRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        if (string.IsNullOrWhiteSpace(request.ActivationId) ||
            string.IsNullOrWhiteSpace(request.ApprovalReference))
        {
            throw Validation(
                "invalid-activation",
                "Activation and approval identities are required.");
        }

        if (request.ReplacedStateStatus is not
            (GovernedDecisionStateStatus.Superseded or
             GovernedDecisionStateStatus.RolledBack))
        {
            throw Validation(
                "invalid-lifecycle-transition",
                "A replacement must supersede or roll back the previous state.");
        }

        ValidateProposal(request.Proposal);
    }

    private void ValidateProposal(DecisionProposal proposal)
    {
        try
        {
            DecisionProposalValidation.Validate(proposal);
        }
        catch (DecisionProposalValidationException error)
        {
            throw Validation(error.Code, error.Message);
        }

        if (proposal.Context.ExpiresAt is { } expiry &&
            expiry <= _timeProvider.GetUtcNow())
        {
            throw Validation(
                "expired-proposal",
                "The proposal expired before activation.");
        }

    }

    private static void ValidateTransition(GovernedStateTransitionRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        if (string.IsNullOrWhiteSpace(request.TransitionId) ||
            string.IsNullOrWhiteSpace(request.StateId) ||
            request.ExpectedGeneration <= 0)
        {
            throw Validation(
                "invalid-lifecycle-transition",
                "Transition identity, state identity, and generation are required.");
        }

        ValidateAddress(request.Address);
        if (request.Status is not
            (GovernedDecisionStateStatus.Expired or
             GovernedDecisionStateStatus.Completed))
        {
            throw Validation(
                "invalid-lifecycle-transition",
                "Only completion and expiry may deactivate authority without a replacement.");
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

    private static NumericRuleStrategy CloneStrategy(
        NumericRuleStrategy strategy) =>
        strategy with
        {
            WeightedInputs = strategy.WeightedInputs?.ToArray()
        };

    private static GovernedStateAddress Address(DecisionProposalContext context) =>
        new(
            context.Definition.AppId,
            context.Definition.Environment,
            context.Definition.DecisionKey,
            context.ControlTarget);

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

internal sealed record GovernedStateTransitionReplay(
    string TransitionId,
    string Fingerprint,
    string StateId);

internal sealed record GovernedStatePersistenceSnapshot(
    int Version,
    IReadOnlyList<GovernedStateEntry> States,
    IReadOnlyList<GovernedStateActivationReplay> Activations,
    IReadOnlyList<GovernedStateTransitionReplay> Transitions,
    LifecycleAuditTrail Journal);
