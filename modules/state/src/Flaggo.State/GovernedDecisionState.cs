using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed record NumericRuleInput(
    string InputKey,
    double Minimum,
    double Maximum,
    double Weight);

public sealed record NumericRuleStrategy(
    string InputKey,
    double Threshold,
    double ValueAtOrAbove,
    double ValueBelow,
    IReadOnlyList<NumericRuleInput>? WeightedInputs = null);

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

public interface IStateStore
{
    Task<GovernedDecisionState?> GetActiveAsync(
        string decisionKey,
        string definitionId,
        string revision,
        IReadOnlyList<DecisionTargetRef?> resolutionTargets,
        CancellationToken cancellationToken);
}

public interface IStateHealth
{
    Task<bool> IsAvailableAsync(CancellationToken cancellationToken);
}

public sealed record IdempotentDecisionResult(
    DecideTerminalOutcome Outcome,
    DateTimeOffset? ExpiresAt);

public enum DecideIdempotencyRetention
{
    Release,
    Retain
}

public static class DecideIdempotencyRetentionPolicy
{
    public static DecideIdempotencyRetention Classify(DecideTerminalOutcome outcome)
    {
        if (outcome.Result is not null)
        {
            return DecideIdempotencyRetention.Retain;
        }

        return outcome.Failure?.Status is >= 400 and < 500
            ? DecideIdempotencyRetention.Retain
            : DecideIdempotencyRetention.Release;
    }
}

public sealed class IdempotencyConflictException : Exception;

public sealed class IdempotencyInProgressException : Exception;

public interface IDecideIdempotencyStore
{
    Task<IdempotentDecisionResult> ExecuteAsync(
        string idempotencyNamespace,
        string key,
        string fingerprint,
        Func<CancellationToken, Task<DecideTerminalOutcome>> operation,
        CancellationToken cancellationToken);
}

public sealed class InMemoryDecideIdempotencyStore : IDecideIdempotencyStore
{
    private sealed record Entry(
        string Fingerprint,
        Task<IdempotentDecisionResult> Completion);

    private readonly Dictionary<(string Namespace, string Key), Entry> _entries = [];
    private readonly object _gate = new();
    private readonly TimeProvider _timeProvider;
    private readonly TimeSpan _followerWaitBudget;
    private readonly Action? _retryableCompletionPublished;

    public InMemoryDecideIdempotencyStore(
        TimeProvider timeProvider,
        TimeSpan? followerWaitBudget = null)
        : this(timeProvider, followerWaitBudget, retryableCompletionPublished: null)
    {
    }

    internal InMemoryDecideIdempotencyStore(
        TimeProvider timeProvider,
        TimeSpan? followerWaitBudget,
        Action? retryableCompletionPublished)
    {
        _timeProvider = timeProvider;
        _followerWaitBudget = followerWaitBudget ?? TimeSpan.FromSeconds(1);
        _retryableCompletionPublished = retryableCompletionPublished;
    }

    public Task<IdempotentDecisionResult> ExecuteAsync(
        string idempotencyNamespace,
        string key,
        string fingerprint,
        Func<CancellationToken, Task<DecideTerminalOutcome>> operation,
        CancellationToken cancellationToken)
    {
        TaskCompletionSource<IdempotentDecisionResult>? owner = null;
        Task<IdempotentDecisionResult> completion;

        lock (_gate)
        {
            var now = _timeProvider.GetUtcNow();
            foreach (var expiredKey in _entries
                         .Where(item =>
                             item.Value.Completion.IsCompletedSuccessfully &&
                             item.Value.Completion.Result.ExpiresAt is { } expiresAt &&
                             expiresAt <= now)
                         .Select(item => item.Key)
                         .ToArray())
            {
                _entries.Remove(expiredKey);
            }

            var entryKey = (idempotencyNamespace, key);
            if (_entries.TryGetValue(entryKey, out var existing))
            {
                if (existing.Completion.IsCompletedSuccessfully &&
                    existing.Completion.Result.ExpiresAt is { } expiresAt &&
                    expiresAt <= _timeProvider.GetUtcNow())
                {
                    _entries.Remove(entryKey);
                    existing = null;
                }
            }

            if (existing is not null)
            {
                if (!string.Equals(existing.Fingerprint, fingerprint, StringComparison.Ordinal))
                {
                    throw new IdempotencyConflictException();
                }

                completion = existing.Completion;
            }
            else
            {
                owner = new TaskCompletionSource<IdempotentDecisionResult>(
                    TaskCreationOptions.RunContinuationsAsynchronously);
                completion = owner.Task;
                _entries.Add(entryKey, new Entry(fingerprint, completion));
            }
        }

        return owner is null
            ? AwaitFollowerAsync(completion, cancellationToken)
            : CompleteOwnerAsync(
                idempotencyNamespace,
                key,
                owner,
                operation,
                cancellationToken);
    }

    private async Task<IdempotentDecisionResult> AwaitFollowerAsync(
        Task<IdempotentDecisionResult> completion,
        CancellationToken cancellationToken)
    {
        try
        {
            return await completion.WaitAsync(_followerWaitBudget, cancellationToken);
        }
        catch (TimeoutException)
        {
            throw new IdempotencyInProgressException();
        }
    }

    private async Task<IdempotentDecisionResult> CompleteOwnerAsync(
        string idempotencyNamespace,
        string key,
        TaskCompletionSource<IdempotentDecisionResult> owner,
        Func<CancellationToken, Task<DecideTerminalOutcome>> operation,
        CancellationToken cancellationToken)
    {
        try
        {
            var result = await operation(cancellationToken);
            var retention = DecideIdempotencyRetentionPolicy.Classify(result);
            var completed = new IdempotentDecisionResult(
                result,
                retention == DecideIdempotencyRetention.Retain
                    ? _timeProvider.GetUtcNow().AddHours(24)
                    : null);
            if (retention == DecideIdempotencyRetention.Release)
            {
                lock (_gate)
                {
                    owner.TrySetResult(completed);
                    _retryableCompletionPublished?.Invoke();
                    _entries.Remove((idempotencyNamespace, key));
                }
            }
            else
            {
                owner.TrySetResult(completed);
            }
            return completed;
        }
        catch (Exception error)
        {
            lock (_gate)
            {
                owner.TrySetException(error);
                _entries.Remove((idempotencyNamespace, key));
            }
            throw;
        }
    }
}

public sealed record PendingExposure(
    string DecisionId,
    string ConfirmToken,
    DecisionSnapshot Snapshot,
    string? AppliedAt,
    ExposureConfirmationResult? Confirmation,
    string? PreparedAppliedAt = null,
    ExposureConfirmationResult? PreparedConfirmation = null);

public sealed record ExposureConfirmationOutcome(
    ExposureConfirmationResult Result,
    DecisionSnapshot Snapshot,
    string? AppliedAt);

public sealed record ExposureConfirmationPreparation(
    ExposureConfirmationOutcome Outcome,
    bool AlreadyConfirmed);

public sealed record DecisionSnapshot(
    string TenantId,
    string AppId,
    string Environment,
    RuntimeContractIdentity Contract,
    JsonElement Value,
    string ValueType,
    ServerFallbackInfo Fallback,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    IReadOnlyDictionary<string, JsonElement> Inputs,
    DecisionTargetRef? RuntimeTarget,
    DecisionTargetRef? ControlTarget,
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    IReadOnlyList<string> ResolutionChain,
    PolicyEvaluationResult Policy,
    DecisionEvidenceSnapshot? Evidence = null,
    ConfidenceReport? Confidence = null,
    IReadOnlyDictionary<string, JsonElement>? RequestInputs = null,
    IReadOnlyDictionary<string, InputProvenance>? InputProvenance = null,
    IReadOnlyList<DecisionTargetRef>? ResolvedTargets = null);

public sealed class ExposureNotFoundException : Exception;

public sealed class ExposureConfirmationConflictException : Exception;

public interface IExposureStore
{
    Task CreatePendingAsync(
        string decisionId,
        string confirmToken,
        DecisionSnapshot snapshot,
        CancellationToken cancellationToken);

    Task RemovePendingAsync(
        string decisionId,
        CancellationToken cancellationToken);

    Task<ExposureConfirmationOutcome?> FindReplayAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);

    Task<ExposureConfirmationPreparation> PrepareConfirmationAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);

    Task CommitConfirmationAsync(
        string decisionId,
        string exposureId,
        CancellationToken cancellationToken);
}

public sealed record ConfirmedExposure(
    ExposureConfirmationResult Confirmation,
    DecisionSnapshot Snapshot);

public interface IConfirmedExposureReader
{
    Task<ConfirmedExposure?> FindConfirmedAsync(
        string exposureId,
        ApplicationScope scope,
        CancellationToken cancellationToken);
}

public sealed class InMemoryExposureStore(
    TimeProvider timeProvider,
    Func<string> createExposureId) : IExposureStore, IConfirmedExposureReader
{
    private readonly Dictionary<string, PendingExposure> _exposures = [];
    private readonly Dictionary<string, string> _confirmedIds = new(StringComparer.Ordinal);
    private readonly object _gate = new();

    public PendingExposure? Find(string decisionId)
    {
        lock (_gate)
        {
            return _exposures.GetValueOrDefault(decisionId);
        }
    }

    public Task CreatePendingAsync(
        string decisionId,
        string confirmToken,
        DecisionSnapshot snapshot,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            _exposures.Add(
                decisionId,
                new PendingExposure(decisionId, confirmToken, snapshot, null, null));
        }

        return Task.CompletedTask;
    }

    public Task RemovePendingAsync(
        string decisionId,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            _exposures.Remove(decisionId);
        }

        return Task.CompletedTask;
    }

    public Task<ExposureConfirmationPreparation> PrepareConfirmationAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            if (!_exposures.TryGetValue(decisionId, out var exposure) ||
                exposure.Snapshot.TenantId != tenantId ||
                !appIds.Contains(exposure.Snapshot.AppId) ||
                !environments.Contains(exposure.Snapshot.Environment) ||
                !string.Equals(exposure.ConfirmToken, request.ConfirmToken, StringComparison.Ordinal))
            {
                throw new ExposureNotFoundException();
            }

            if (exposure.Confirmation is not null)
            {
                if (!string.Equals(exposure.AppliedAt, request.AppliedAt, StringComparison.Ordinal))
                {
                    throw new ExposureConfirmationConflictException();
                }

                return Task.FromResult(
                    new ExposureConfirmationPreparation(
                        new ExposureConfirmationOutcome(
                            exposure.Confirmation,
                            exposure.Snapshot,
                            exposure.AppliedAt),
                        true));
            }

            if (exposure.PreparedConfirmation is not null)
            {
                if (!string.Equals(
                        exposure.PreparedAppliedAt,
                        request.AppliedAt,
                        StringComparison.Ordinal))
                {
                    throw new ExposureConfirmationConflictException();
                }

                return Task.FromResult(
                    new ExposureConfirmationPreparation(
                        new ExposureConfirmationOutcome(
                            exposure.PreparedConfirmation,
                            exposure.Snapshot,
                            exposure.PreparedAppliedAt),
                        false));
            }

            var result = new ExposureConfirmationResult(
                createExposureId(),
                decisionId,
                "confirmed",
                timeProvider.GetUtcNow().UtcDateTime.ToString("O"));
            _exposures[decisionId] = exposure with
            {
                PreparedAppliedAt = request.AppliedAt,
                PreparedConfirmation = result
            };
            return Task.FromResult(
                new ExposureConfirmationPreparation(
                    new ExposureConfirmationOutcome(
                        result,
                        exposure.Snapshot,
                        request.AppliedAt),
                    false));
        }
    }

    public Task CommitConfirmationAsync(
        string decisionId,
        string exposureId,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            if (!_exposures.TryGetValue(decisionId, out var exposure))
            {
                throw new ExposureNotFoundException();
            }

            if (exposure.Confirmation is not null)
            {
                if (!string.Equals(
                        exposure.Confirmation.ExposureId,
                        exposureId,
                        StringComparison.Ordinal))
                {
                    throw new ExposureConfirmationConflictException();
                }

                return Task.CompletedTask;
            }

            if (exposure.PreparedConfirmation is null ||
                !string.Equals(
                    exposure.PreparedConfirmation.ExposureId,
                    exposureId,
                    StringComparison.Ordinal))
            {
                throw new ExposureConfirmationConflictException();
            }

            if (_confirmedIds.TryGetValue(exposureId, out var owner) && owner != decisionId)
            {
                throw new ExposureConfirmationConflictException();
            }
            _exposures[decisionId] = exposure with
            {
                AppliedAt = exposure.PreparedAppliedAt,
                Confirmation = exposure.PreparedConfirmation,
                PreparedAppliedAt = null,
                PreparedConfirmation = null
            };
            _confirmedIds.Add(exposureId, decisionId);
            return Task.CompletedTask;
        }
    }

    public Task<ConfirmedExposure?> FindConfirmedAsync(
        string exposureId,
        ApplicationScope scope,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            if (!_confirmedIds.TryGetValue(exposureId, out var decisionId) ||
                !_exposures.TryGetValue(decisionId, out var exposure) ||
                exposure.Confirmation is null ||
                exposure.Snapshot.TenantId != scope.TenantId ||
                exposure.Snapshot.AppId != scope.AppId ||
                exposure.Snapshot.Environment != scope.Environment)
            {
                return Task.FromResult<ConfirmedExposure?>(null);
            }
            return Task.FromResult<ConfirmedExposure?>(
                new ConfirmedExposure(exposure.Confirmation, exposure.Snapshot));
        }
    }

    public Task<ExposureConfirmationOutcome?> FindReplayAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            if (!_exposures.TryGetValue(decisionId, out var exposure) ||
                exposure.Snapshot.TenantId != tenantId ||
                !appIds.Contains(exposure.Snapshot.AppId) ||
                !environments.Contains(exposure.Snapshot.Environment) ||
                !string.Equals(exposure.ConfirmToken, request.ConfirmToken, StringComparison.Ordinal))
            {
                throw new ExposureNotFoundException();
            }

            if (exposure.Confirmation is null &&
                exposure.PreparedConfirmation is null)
            {
                return Task.FromResult<ExposureConfirmationOutcome?>(null);
            }

            var appliedAt = exposure.Confirmation is not null
                ? exposure.AppliedAt
                : exposure.PreparedAppliedAt;
            var confirmation = exposure.Confirmation ??
                exposure.PreparedConfirmation!;
            if (!string.Equals(appliedAt, request.AppliedAt, StringComparison.Ordinal))
            {
                throw new ExposureConfirmationConflictException();
            }

            return Task.FromResult<ExposureConfirmationOutcome?>(
                new ExposureConfirmationOutcome(
                    confirmation,
                    exposure.Snapshot,
                    appliedAt));
        }
    }
}

public sealed class InMemoryStateStore(
    IEnumerable<(string DecisionKey, GovernedDecisionState State)> states,
    bool available = true) : IStateStore, IStateHealth
{
    private readonly IReadOnlyList<(string DecisionKey, GovernedDecisionState State)> _states =
        states.ToArray();

    public Task<GovernedDecisionState?> GetActiveAsync(
        string decisionKey,
        string definitionId,
        string revision,
        IReadOnlyList<DecisionTargetRef?> resolutionTargets,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var state = resolutionTargets
            .Select(target => _states.FirstOrDefault(item =>
                string.Equals(item.DecisionKey, decisionKey, StringComparison.Ordinal) &&
                string.Equals(item.State.DefinitionId, definitionId, StringComparison.Ordinal) &&
                string.Equals(item.State.Revision, revision, StringComparison.Ordinal) &&
                TargetsEqual(item.State.ControlTarget, target)).State)
            .FirstOrDefault(candidate => candidate is not null);
        return Task.FromResult(state);
    }

    private static bool TargetsEqual(DecisionTargetRef? left, DecisionTargetRef? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        string.Equals(left.Type, right.Type, StringComparison.Ordinal) &&
        string.Equals(left.Id, right.Id, StringComparison.Ordinal);

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}
