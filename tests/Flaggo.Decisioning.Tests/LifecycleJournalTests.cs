using Flaggo.Audit;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LifecycleJournalTests
{
    [Fact]
    public async Task ReviewApprovalAndActivationAreReconstructableAndRuntimeReadable()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        var review = await LifecycleTestData.ReviewAsync(store);
        Assert.Equal(LifecycleDisposition.Approved, review.Disposition);
        Assert.Equal("automatic", review.Approval!.Mode);
        Assert.Null(await store.GetBaselineAsync(Address(), CancellationToken.None));

        var activated = await LifecycleTestData.ActivateAsync(store);
        var runtime = new GovernedStateRuntimeProjection(store, "app", "dev");
        var state = await runtime.GetActiveAsync(
            "decision", "definition", "revision",
            [LifecycleTestData.Target], CancellationToken.None);
        var audit = await ((ILifecycleAuditReader)store).ReadAsync(
            "app", "dev", CancellationToken.None);

        Assert.Equal(LifecycleMutationStatus.Applied, activated.Status);
        Assert.Equal(activated.StateId, state!.StateId);
        Assert.Equal(review.Approval.ApprovalId, state.ApprovalReference);
        Assert.Equal(800, state.Value.GetInt32());
        Assert.Equal(
            [LifecycleAuditKind.Proposal, LifecycleAuditKind.Review,
                LifecycleAuditKind.AutomaticApproval, LifecycleAuditKind.Activation],
            audit.Records.Select(record => record.Kind));
        Assert.Equal(activated.AuditId, audit.Records.Last().AuditId);
        Assert.Equal(review.ReviewDigest, audit.Reviews.Single().Receipt.ReviewDigest);
    }

    [Fact]
    public async Task HumanRequiredReviewCannotBeActivated()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        var policy = LifecycleTestData.Policy with
        {
            OperatorControls = LifecycleTestData.Layer() with { AutomaticApprovalAllowed = false }
        };
        var review = await LifecycleTestData.ReviewAsync(store, policy: policy);
        var activation = await LifecycleTestData.ActivateAsync(store);

        Assert.Equal(LifecycleDisposition.PendingApproval, review.Disposition);
        Assert.Null(review.Approval);
        Assert.Equal(LifecycleMutationStatus.Rejected, activation.Status);
        Assert.Contains("review_not_approved", activation.Reasons);
        Assert.Null(await store.GetBaselineAsync(Address(), CancellationToken.None));
    }

    [Fact]
    public async Task ExactReviewAndActivationRetriesDoNotDuplicateAudit()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        var review = await LifecycleTestData.ReviewAsync(store);
        var activation = await LifecycleTestData.ActivateAsync(store);
        var replayedReview = await LifecycleTestData.ReviewAsync(store);
        var replayedActivation = await LifecycleTestData.ActivateAsync(store);

        Assert.Equal(LifecycleJson.Digest(review), LifecycleJson.Digest(replayedReview));
        Assert.Equal(LifecycleJson.Digest(activation), LifecycleJson.Digest(replayedActivation));
        var audit = await store.ReadAsync("app", "dev", CancellationToken.None);
        Assert.Equal(4, audit.Records.Count);
    }

    [Fact]
    public async Task ConflictingReviewAndActivationKeysFailExplicitly()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        await LifecycleTestData.ReviewAsync(store);
        var reviewConflict = await Assert.ThrowsAsync<GovernedStateConflictException>(() =>
            LifecycleTestData.ReviewAsync(store, LifecycleTestData.Proposal(value: 850)));
        Assert.Equal("review-conflict", reviewConflict.Code);

        await LifecycleTestData.ActivateAsync(store);
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("proposal-2"), "review-2");
        var activationConflict = await Assert.ThrowsAsync<GovernedStateConflictException>(() =>
            LifecycleTestData.ActivateAsync(store, "review-2"));
        Assert.Equal("activation-conflict", activationConflict.Code);
    }

    [Fact]
    public async Task ConcurrentApprovedReplacementsHaveExactlyOneWinner()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        await LifecycleTestData.ReviewAsync(store);
        var initial = await LifecycleTestData.ActivateAsync(store);
        var baseline = new GovernedStateBaseline(initial.StateId, initial.Generation!.Value);
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("proposal-2", 850, baseline), "review-2");
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("proposal-3", 750, baseline), "review-3");

        var results = await Task.WhenAll(
            LifecycleTestData.ActivateAsync(store, "review-2", "activation-2"),
            LifecycleTestData.ActivateAsync(store, "review-3", "activation-3"));

        Assert.Single(results.Where(result => result.Status == LifecycleMutationStatus.Applied));
        var loser = Assert.Single(results.Where(result => result.Status == LifecycleMutationStatus.Rejected));
        Assert.Contains("stale-baseline", loser.Reasons);
        var current = await store.GetBaselineAsync(Address(), CancellationToken.None);
        Assert.Equal(2, current!.Generation);
        var originalReplay = await LifecycleTestData.ActivateAsync(store);
        Assert.Equal(LifecycleJson.Digest(initial), LifecycleJson.Digest(originalReplay));
        Assert.Equal(GovernedDecisionStateStatus.Superseded,
            (await store.GetStateAsync(initial.StateId!, CancellationToken.None))!.LifecycleStatus);
    }

    [Fact]
    public async Task ReturnedObjectsCannotMutateTheStoredApproval()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        var receipt = await LifecycleTestData.ReviewAsync(store);
        var kinds = Assert.IsAssignableFrom<IList<string>>(receipt.EffectivePolicy!.AllowedTargetKinds);
        kinds[0] = "unauthorized";
        var loaded = await store.GetReviewAsync("review-1", CancellationToken.None);
        Assert.DoesNotContain("unauthorized", loaded!.Receipt.EffectivePolicy!.AllowedTargetKinds);
        var result = await LifecycleTestData.ActivateAsync(store);
        Assert.Equal(LifecycleMutationStatus.Applied, result.Status);
    }

    [Fact]
    public async Task AuditValidationFailureDoesNotPublishAPartialJournal()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        await LifecycleTestData.ReviewAsync(store);
        var activated = await LifecycleTestData.ActivateAsync(store);
        var review = (await store.GetReviewAsync("review-1", CancellationToken.None))!;
        var before = await store.ReadAsync("app", "dev", CancellationToken.None);
        var invalid = review.Commit with
        {
            Request = review.Commit.Request with { ReviewId = "invalid-review" },
            Decision = new LifecyclePolicyDecision(LifecycleDisposition.Hold, [], null)
        };

        await Assert.ThrowsAsync<InvalidDataException>(() =>
            store.CommitReviewAsync(invalid, CancellationToken.None));

        Assert.Null(await store.GetReviewAsync("invalid-review", CancellationToken.None));
        Assert.Equal(activated.StateId,
            (await store.GetBaselineAsync(Address(), CancellationToken.None))!.StateId);
        Assert.Equal(LifecycleJson.Digest(before),
            LifecycleJson.Digest(await store.ReadAsync("app", "dev", CancellationToken.None)));
    }

    [Fact]
    public async Task TerminalAuditPreservesTheFullActivatedDefinitionIdentity()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                Definition = LifecycleTestData.Definition with
                {
                    Contract = LifecycleTestData.Definition.Contract with
                    {
                        BundleDigest = $"sha256:{new string('b', 64)}"
                    }
                }
            }
        };
        await LifecycleTestData.ReviewAsync(store, proposal);
        var activated = await LifecycleTestData.ActivateAsync(store);
        var terminal = await store.CommitTransitionAsync(
            new(new("complete", Address(), activated.StateId!, activated.Generation!.Value,
                    GovernedDecisionStateStatus.Completed, "Finished the governed run."),
                LifecycleTestData.Actor), CancellationToken.None);
        var audit = (await store.ReadAsync("app", "dev", CancellationToken.None))
            .Records.Single(record => record.AuditId == terminal.AuditId);

        Assert.Equal(proposal.Context.Definition, audit.Definition);
    }

    [Fact]
    public async Task CancellationCannotCommitAReviewOrAuthority()
    {
        var store = new InMemoryGovernedStateLifecycleStore(new TestClock());
        await LifecycleTestData.ReviewAsync(store);
        var record = await store.GetReviewAsync("review-1", CancellationToken.None);
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            store.CommitActivationAsync(
                new LifecycleActivationCommit(
                    new LifecycleActivationRequest("activation-1", "review-1"),
                    LifecycleTestData.Actor,
                    LifecycleIdentity.InputsDigest(record!.Commit),
                    record.Commit.Decision),
                cancellation.Token));
        Assert.Null(await store.GetBaselineAsync(Address(), CancellationToken.None));
        Assert.Equal(3, (await store.ReadAsync("app", "dev", CancellationToken.None)).Records.Count);
    }

    private static GovernedStateAddress Address() =>
        new("app", "dev", "decision", LifecycleTestData.Target);

    private sealed class TestClock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }
}
