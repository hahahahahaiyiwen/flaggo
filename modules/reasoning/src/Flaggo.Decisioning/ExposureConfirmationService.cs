using Flaggo.Audit;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging;

namespace Flaggo.Decisioning;

public interface IExposureConfirmationService
{
    Task<ExposureConfirmationOutcome> ConfirmAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken);
}

public interface IPostAuditExposureCommitPolicy
{
    Task ExecuteAsync(Func<CancellationToken, Task> commit);
}

public sealed class BoundedPostAuditExposureCommitPolicy(
    TimeSpan timeout,
    TimeProvider timeProvider,
    CancellationToken shutdownToken,
    ILogger<BoundedPostAuditExposureCommitPolicy> logger)
    : IPostAuditExposureCommitPolicy
{
    public Task ExecuteAsync(Func<CancellationToken, Task> commit)
    {
        ArgumentNullException.ThrowIfNull(commit);
        if (timeout <= TimeSpan.Zero || timeout == Timeout.InfiniteTimeSpan)
        {
            throw new InvalidOperationException(
                "The post-audit exposure commit timeout must be finite and positive.");
        }

        return ExecuteCoreAsync(commit);
    }

    private async Task ExecuteCoreAsync(Func<CancellationToken, Task> commit)
    {
        using var timeoutSource = new CancellationTokenSource(timeout, timeProvider);
        using var commitSource = CancellationTokenSource.CreateLinkedTokenSource(
            timeoutSource.Token,
            shutdownToken);
        var commitTask = commit(commitSource.Token);
        try
        {
            await commitTask.WaitAsync(timeout, timeProvider, shutdownToken);
        }
        catch (TimeoutException exception)
        {
            ObserveLateCompletion(commitTask);
            throw new TimeoutException(
                $"The post-audit exposure commit exceeded its {timeout} timeout.",
                exception);
        }
        catch (OperationCanceledException)
            when (shutdownToken.IsCancellationRequested)
        {
            ObserveLateCompletion(commitTask);
            throw;
        }
        catch (OperationCanceledException exception)
            when (timeoutSource.IsCancellationRequested &&
                  !shutdownToken.IsCancellationRequested)
        {
            ObserveLateCompletion(commitTask);
            throw new TimeoutException(
                $"The post-audit exposure commit exceeded its {timeout} timeout.",
                exception);
        }
    }

    private void ObserveLateCompletion(Task commitTask)
    {
        if (commitTask.IsCompleted)
        {
            ObserveCompletion(commitTask);
            return;
        }

        _ = commitTask.ContinueWith(
            static (completedTask, state) =>
                ((BoundedPostAuditExposureCommitPolicy)state!)
                    .ObserveCompletion(completedTask),
            this,
            CancellationToken.None,
            TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
    }

    private void ObserveCompletion(Task commitTask)
    {
        if (!commitTask.IsFaulted)
        {
            return;
        }

        var exception = commitTask.Exception;
        try
        {
            logger.LogError(
                exception,
                "Post-audit exposure commit failed after its bounded wait ended.");
        }
        catch
        {
            // The commit exception is already observed; logging cannot be allowed
            // to create another unobserved continuation failure.
        }
    }
}

public sealed class ExposureConfirmationService(
    IExposureStore exposureStore,
    IExposureAuditSink exposureAuditSink,
    IPostAuditExposureCommitPolicy postAuditCommitPolicy)
    : IExposureConfirmationService
{
    public async Task<ExposureConfirmationOutcome> ConfirmAsync(
        string tenantId,
        string decisionId,
        ExposureConfirmationRequest request,
        IReadOnlySet<string> appIds,
        IReadOnlySet<string> environments,
        CancellationToken cancellationToken)
    {
        var preparation = await exposureStore.PrepareConfirmationAsync(
            tenantId,
            decisionId,
            request,
            appIds,
            environments,
            cancellationToken);
        var outcome = preparation.Outcome;
        if (preparation.AlreadyConfirmed)
        {
            return outcome;
        }

        await exposureAuditSink.RecordExposureAsync(
            new ExposureAuditRecord(
                outcome.Result.ExposureId,
                outcome.Result.DecisionId,
                outcome.Snapshot.AppId,
                outcome.Snapshot.Environment,
                outcome.AppliedAt,
                outcome.Result.ConfirmedAt),
            cancellationToken);
        await postAuditCommitPolicy.ExecuteAsync(
            commitCancellationToken => exposureStore.CommitConfirmationAsync(
                decisionId,
                outcome.Result.ExposureId,
                commitCancellationToken));

        return outcome;
    }
}
