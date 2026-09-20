using System.Text.Json;
using Flaggo.Evidence;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging;

namespace Flaggo.Lifecycle;

public interface ILifecycleActorProvider
{
    Task<LifecycleActor> GetAsync(
        string appId,
        string environment,
        CancellationToken cancellationToken);
}

public interface IProposalGovernance
{
    Task<LifecycleReviewReceipt> ReviewAsync(
        LifecycleReviewRequest request,
        CancellationToken cancellationToken);

    Task<LifecycleActivationReceipt> ActivateAsync(
        LifecycleActivationRequest request,
        CancellationToken cancellationToken);
}

public sealed class LifecycleGovernanceException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

public sealed record LifecycleCommitOptions(
    TimeSpan Timeout,
    CancellationToken Shutdown = default);

public sealed class ProposalGovernance(
    IRuntimeDefinitionReader runtimeDefinitions,
    IIntelligenceDefinitionReader intelligenceDefinitions,
    ILifecyclePolicyContextProvider policyContexts,
    IProposalEvidenceReader evidence,
    ILifecycleActorProvider actors,
    ILifecyclePolicyEvaluator policy,
    IGovernedStateLifecycleStore state,
    TimeProvider clock,
    ILogger<ProposalGovernance> logger,
    LifecycleCommitOptions? options = null) : IProposalGovernance
{
    private readonly LifecycleCommitOptions _commitOptions = ValidateOptions(options);

    public Task<LifecycleReviewReceipt> ReviewAsync(
        LifecycleReviewRequest request,
        CancellationToken cancellationToken) =>
        GuardedAsync("review", request?.ReviewId, () => ReviewCoreAsync(request!, cancellationToken));

    public Task<LifecycleActivationReceipt> ActivateAsync(
        LifecycleActivationRequest request,
        CancellationToken cancellationToken) =>
        GuardedAsync("activation", request?.ActivationId, () => ActivateCoreAsync(request!, cancellationToken));

    private async Task<LifecycleReviewReceipt> ReviewCoreAsync(
        LifecycleReviewRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (request is null || string.IsNullOrWhiteSpace(request.ReviewId))
        {
            throw new LifecycleGovernanceException("invalid-review", "A review identity is required.");
        }
        DecisionProposalValidation.Validate(request.Proposal);
        if (request.Proposal.Context.ControlTarget is null)
        {
            throw new LifecycleGovernanceException(
                "control-target-required", "Lifecycle proposals require an explicit control target.");
        }
        request = LifecycleJson.Copy(request);
        var definition = request.Proposal.Context.Definition;
        var actor = await ActorAsync(definition, review: true, cancellationToken);
        var prior = await state.GetReviewAsync(request.ReviewId, cancellationToken);
        if (prior is not null)
        {
            RequireFingerprint(
                prior.RequestFingerprint,
                LifecycleIdentity.ReviewRequest(request, actor.Identity),
                "review-conflict");
            return prior.Receipt;
        }

        var commit = await ResolveReviewAsync(request, actor, cancellationToken);
        return await CommitAsync(
            token => state.CommitReviewAsync(commit, token),
            cancellationToken);
    }

    private async Task<LifecycleActivationReceipt> ActivateCoreAsync(
        LifecycleActivationRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (request is null || string.IsNullOrWhiteSpace(request.ActivationId) ||
            string.IsNullOrWhiteSpace(request.ReviewId))
        {
            throw new LifecycleGovernanceException(
                "invalid-activation", "Activation and review identities are required.");
        }
        var prior = await state.GetActivationAsync(request.ActivationId, cancellationToken);
        if (prior is not null)
        {
            var replayActor = await ActorAsync(prior.Definition, review: false, cancellationToken);
            RequireFingerprint(
                prior.Fingerprint,
                LifecycleIdentity.ActivationRequest(request, replayActor.Identity),
                "activation-conflict");
            return prior;
        }

        var review = await state.GetReviewAsync(request.ReviewId, cancellationToken)
            ?? throw new LifecycleGovernanceException("review-not-found", "The proposal review does not exist.");
        var definition = review.Commit.Request.Proposal.Context.Definition;
        var actor = await ActorAsync(definition, review: false, cancellationToken);
        var fresh = review.Receipt.Approval is null
            ? review.Commit
            : await ResolveReviewAsync(review.Commit.Request, actor, cancellationToken);
        var commit = new LifecycleActivationCommit(
            request, actor, LifecycleIdentity.InputsDigest(fresh), fresh.Decision);
        return await CommitAsync(
            token => state.CommitActivationAsync(commit, token),
            cancellationToken);
    }

    private async Task<LifecycleReviewCommit> ResolveReviewAsync(
        LifecycleReviewRequest request,
        LifecycleActor actor,
        CancellationToken cancellationToken)
    {
        var proposal = request.Proposal;
        var context = proposal.Context;
        var definition = context.Definition;
        var runtime = (await runtimeDefinitions.ResolveRuntimeAsync(
            definition.AppId, definition.Environment, definition.DecisionKey,
            definition.Contract.DefinitionId, definition.Contract.Revision, cancellationToken)).Definition;
        var intelligence = (await intelligenceDefinitions.ResolveIntelligenceAsync(
            definition.AppId, definition.Environment, definition.DecisionKey,
            definition.Contract.DefinitionId, definition.Contract.Revision, cancellationToken)).Definition;
        if (runtime is null || intelligence is null)
        {
            throw new LifecycleGovernanceException(
                "definition-not-found", "Both registered lifecycle and runtime definition projections are required.");
        }

        var baseline = await state.GetBaselineAsync(
            new GovernedStateAddress(
                definition.AppId, definition.Environment, definition.DecisionKey, context.ControlTarget),
            cancellationToken);
        var policyContext = await policyContexts.GetAsync(definition, cancellationToken);
        var references = context.EvidenceReferences.Concat(context.ConfidenceReferences)
            .Distinct(StringComparer.Ordinal).Order(StringComparer.Ordinal).ToArray();
        var snapshots = await evidence.GetAsync(
            new ProposalEvidenceRequest(definition, context.ControlTarget!, references),
            cancellationToken);
        var decision = await policy.EvaluateAsync(
            new LifecyclePolicyEvaluationRequest(
                proposal, request.Replacement, actor, runtime, intelligence,
                policyContext, snapshots, baseline, clock.GetUtcNow()),
            cancellationToken);
        return new LifecycleReviewCommit(
            request, actor, decision,
            LifecycleJson.Digest(new { Runtime = runtime, Intelligence = intelligence }),
            policyContext, snapshots, baseline);
    }

    private async Task<LifecycleActor> ActorAsync(
        GovernedDefinitionIdentity definition,
        bool review,
        CancellationToken cancellationToken)
    {
        var actor = await actors.GetAsync(definition.AppId, definition.Environment, cancellationToken);
        if (actor is null ||
            string.IsNullOrWhiteSpace(actor.Subject) || string.IsNullOrWhiteSpace(actor.Issuer) ||
            actor.AppId != definition.AppId || actor.Environment != definition.Environment ||
            (review ? !actor.CanReview : !actor.CanActivate))
        {
            throw new LifecycleGovernanceException(
                "actor-not-authorized", "The authenticated actor lacks lifecycle authority for this resource.");
        }
        return actor;
    }

    private async Task<T> CommitAsync<T>(
        Func<CancellationToken, Task<T>> commit,
        CancellationToken cancellationToken)
    {
        using var deadline = new CancellationTokenSource(_commitOptions.Timeout, clock);
        using var combined = CancellationTokenSource.CreateLinkedTokenSource(
            cancellationToken, deadline.Token, _commitOptions.Shutdown);
        var pending = commit(combined.Token);
        try
        {
            return await pending.WaitAsync(combined.Token);
        }
        catch (OperationCanceledException) when (
            deadline.IsCancellationRequested &&
            !cancellationToken.IsCancellationRequested &&
            !_commitOptions.Shutdown.IsCancellationRequested)
        {
            ObserveLateFailure(pending);
            throw new TimeoutException(
                "The lifecycle commit deadline elapsed; its outcome may be unknown. Retry the same operation identity.");
        }
        catch (OperationCanceledException error)
        {
            ObserveLateFailure(pending);
            throw new OperationCanceledException(
                "The lifecycle commit wait was canceled; its outcome may be unknown. Retry the same operation identity.",
                error, error.CancellationToken);
        }
    }

    private void ObserveLateFailure(Task pending)
    {
        // The underlying operation retains its writer lease until it actually finishes.
        _ = pending.ContinueWith(
            completed => logger.LogError(
                completed.Exception, "A lifecycle commit failed after its caller stopped waiting."),
            CancellationToken.None,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
    }

    private async Task<T> GuardedAsync<T>(string operation, string? operationId, Func<Task<T>> action)
    {
        try
        {
            return await action();
        }
        catch (Exception error) when (error is LifecycleGovernanceException or
            DecisionProposalValidationException or GovernedStateValidationException or GovernedStateConflictException)
        {
            logger.LogWarning(error, "Lifecycle {Operation} {OperationId} was rejected.", operation, operationId);
            throw;
        }
        catch (OperationCanceledException error)
        {
            logger.LogInformation(error, "Lifecycle {Operation} {OperationId} was canceled; reconcile any commit by its original identity.",
                operation, operationId);
            throw;
        }
        catch (Exception error) when (error is IOException or InvalidDataException or
            TimeoutException or InvalidOperationException or
            UnauthorizedAccessException or EvidenceUnavailableException or JsonException)
        {
            logger.LogError(error, "Lifecycle {Operation} {OperationId} failed; no success was acknowledged.",
                operation, operationId);
            throw;
        }
    }

    private static void RequireFingerprint(string expected, string actual, string code)
    {
        if (expected != actual)
        {
            throw new LifecycleGovernanceException(
                code, "The operation identity is bound to different content or actor identity.");
        }
    }

    private static LifecycleCommitOptions ValidateOptions(LifecycleCommitOptions? supplied)
    {
        var result = supplied ?? new LifecycleCommitOptions(TimeSpan.FromSeconds(5));
        if (result.Timeout <= TimeSpan.Zero || result.Timeout.TotalMilliseconds > uint.MaxValue - 1)
        {
            throw new ArgumentOutOfRangeException(nameof(supplied), "Lifecycle commit timeout must be finite and positive.");
        }
        return result;
    }
}
