using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed record GovernedDecisionState(
    string DefinitionId,
    string Revision,
    string ContractDigest,
    JsonElement Value,
    DecisionTargetRef? ControlTarget = null,
    string Mode = "active-value",
    string? StrategyId = null);

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
    DateTimeOffset ExpiresAt);

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

public sealed class InMemoryDecideIdempotencyStore(
    TimeProvider timeProvider,
    TimeSpan? followerWaitBudget = null) : IDecideIdempotencyStore
{
    private readonly TimeSpan _followerWaitBudget =
        followerWaitBudget ?? TimeSpan.FromSeconds(1);

    private sealed record Entry(
        string Fingerprint,
        Task<IdempotentDecisionResult> Completion);

    private readonly Dictionary<(string Namespace, string Key), Entry> _entries = [];
    private readonly object _gate = new();

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
            var now = timeProvider.GetUtcNow();
            foreach (var expiredKey in _entries
                         .Where(item =>
                             item.Value.Completion.IsCompletedSuccessfully &&
                             item.Value.Completion.Result.ExpiresAt <= now)
                         .Select(item => item.Key)
                         .ToArray())
            {
                _entries.Remove(expiredKey);
            }

            var entryKey = (idempotencyNamespace, key);
            if (_entries.TryGetValue(entryKey, out var existing))
            {
                if (existing.Completion.IsCompletedSuccessfully &&
                    existing.Completion.Result.ExpiresAt <= timeProvider.GetUtcNow())
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
            var retained = new IdempotentDecisionResult(
                result,
                timeProvider.GetUtcNow().AddHours(24));
            owner.TrySetResult(retained);
            return retained;
        }
        catch (Exception error)
        {
            lock (_gate)
            {
                _entries.Remove((idempotencyNamespace, key));
            }

            owner.TrySetException(error);
            throw;
        }
    }
}

public sealed record PendingExposure(
    string DecisionId,
    string ConfirmToken,
    DecisionSnapshot Snapshot,
    string? AppliedAt,
    ExposureConfirmationResult? Confirmation);

public sealed record DecisionSnapshot(
    string AppId,
    string Environment,
    RuntimeContractIdentity Contract,
    JsonElement Value,
    string ValueType,
    ServerFallbackInfo Fallback,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    IReadOnlyList<SignalInput> Inputs,
    DecisionTargetRef? RuntimeTarget,
    DecisionTargetRef? ControlTarget,
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    IReadOnlyList<string> ResolutionChain,
    PolicyEvaluationResult Policy);

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

    Task<ExposureConfirmationResult?> FindReplayAsync(
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);

    Task<ExposureConfirmationResult> ConfirmAsync(
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);
}

public sealed class InMemoryExposureStore(
    TimeProvider timeProvider,
    Func<string> createExposureId) : IExposureStore
{
    private readonly Dictionary<string, PendingExposure> _exposures = [];
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

    public Task<ExposureConfirmationResult> ConfirmAsync(
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

                return Task.FromResult(exposure.Confirmation);
            }

            var result = new ExposureConfirmationResult(
                createExposureId(),
                decisionId,
                "confirmed",
                timeProvider.GetUtcNow().UtcDateTime.ToString("O"));
            _exposures[decisionId] = exposure with
            {
                AppliedAt = request.AppliedAt,
                Confirmation = result
            };
            return Task.FromResult(result);
        }
    }

    public Task<ExposureConfirmationResult?> FindReplayAsync(
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
                !appIds.Contains(exposure.Snapshot.AppId) ||
                !environments.Contains(exposure.Snapshot.Environment) ||
                !string.Equals(exposure.ConfirmToken, request.ConfirmToken, StringComparison.Ordinal))
            {
                throw new ExposureNotFoundException();
            }

            if (exposure.Confirmation is null)
            {
                return Task.FromResult<ExposureConfirmationResult?>(null);
            }

            if (!string.Equals(exposure.AppliedAt, request.AppliedAt, StringComparison.Ordinal))
            {
                throw new ExposureConfirmationConflictException();
            }

            return Task.FromResult<ExposureConfirmationResult?>(exposure.Confirmation);
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
