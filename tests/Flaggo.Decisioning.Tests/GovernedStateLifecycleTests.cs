using System.Text.Json;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class GovernedStateLifecycleTests
{
    [Fact]
    public async Task ApprovedNumericStrategyCreatesSupportedRuntimeState()
    {
        var store = Store();
        var proposal = new NumericStrategyDecisionProposal(
            LifecycleTestData.Proposal().Context,
            JsonSerializer.SerializeToElement(800),
            "strategy-1",
            new NumericRuleStrategy("pressure", 0.5, 850, 750));
        await LifecycleTestData.ReviewAsync(store, proposal);
        var activation = await LifecycleTestData.ActivateAsync(store);
        var state = await store.GetStateAsync(activation.StateId!, CancellationToken.None);

        Assert.Equal("strategy", state!.Mode);
        Assert.Equal("strategy-1", state.StrategyId);
        Assert.Equal("pressure", state.NumericRule!.InputSignalKey);
        Assert.Equal(1, state.Generation);
        Assert.Equal(LifecycleTestData.Now, state.ActivatedAt);
        Assert.Equal(state.ActivatedAt, state.LastChangedAt);
    }

    [Fact]
    public async Task SuccessfulProposalCannotActivateUnderAnotherIdentity()
    {
        var store = Store();
        await LifecycleTestData.ReviewAsync(store);
        await LifecycleTestData.ActivateAsync(store);
        var duplicate = await LifecycleTestData.ActivateAsync(store, activationId: "duplicate");

        Assert.Equal(LifecycleMutationStatus.Rejected, duplicate.Status);
        Assert.Contains("duplicate-proposal", duplicate.Reasons);
        Assert.Null(duplicate.StateId);
    }

    [Fact]
    public async Task StaleBaselineCannotOverwriteNewerAuthority()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        var baseline = new GovernedStateBaseline(first.StateId, first.Generation!.Value);
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("second", 850, baseline), "second");
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("stale", 750, baseline), "stale");
        var second = await LifecycleTestData.ActivateAsync(store, "second", "second");
        var stale = await LifecycleTestData.ActivateAsync(store, "stale", "stale");

        Assert.Contains("stale-baseline", stale.Reasons);
        Assert.Equal(second.StateId,
            (await store.GetBaselineAsync(Address(), CancellationToken.None))!.StateId);
    }

    [Fact]
    public async Task BaselineFromAnotherTargetCannotAuthorizeActivation()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        var proposal = LifecycleTestData.Proposal(
            "other-target", 850, new(first.StateId, first.Generation!.Value));
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                ControlTarget = new DecisionTargetRef("cohort", "other")
            }
        };
        await LifecycleTestData.ReviewAsync(store, proposal, "other");
        var result = await LifecycleTestData.ActivateAsync(store, "other", "other");

        Assert.Equal(LifecycleMutationStatus.Rejected, result.Status);
        Assert.Contains("target-conflict", result.Reasons);
    }

    [Fact]
    public async Task ChangingTheDigestOfAnExistingRevisionCannotOverwriteAuthority()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        var proposal = LifecycleTestData.Proposal(
            "conflicting-definition", 850, new(first.StateId, first.Generation!.Value));
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                Definition = LifecycleTestData.Definition with
                {
                    Contract = LifecycleTestData.Definition.Contract with
                    {
                        ContractDigest = $"sha256:{new string('b', 64)}"
                    }
                }
            }
        };
        await LifecycleTestData.ReviewAsync(store, proposal, "conflict");
        var result = await LifecycleTestData.ActivateAsync(store, "conflict", "conflict");
        Assert.Contains("incompatible-definition", result.Reasons);
    }

    [Fact]
    public async Task UnsupportedAndMalformedProposalsFailBeforePublication()
    {
        var store = Store();
        var unsupported = await Assert.ThrowsAsync<DecisionProposalValidationException>(() =>
            LifecycleTestData.ReviewAsync(
                store, new UnsupportedProposal(LifecycleTestData.Proposal().Context)));
        Assert.Equal("unsupported-state-kind", unsupported.Code);
        var malformed = await Assert.ThrowsAsync<DecisionProposalValidationException>(() =>
            LifecycleTestData.ReviewAsync(
                store, LifecycleTestData.Proposal() with
                {
                    Value = JsonSerializer.SerializeToElement(new { invalid = true })
                }));
        Assert.Equal("invalid-proposal", malformed.Code);
        Assert.Empty((await store.ReadAsync("app", "dev", CancellationToken.None)).Records);
    }

    [Theory]
    [InlineData(GovernedDecisionStateStatus.Completed)]
    [InlineData(GovernedDecisionStateStatus.Expired)]
    public async Task TerminalTransitionIsAuditedAndCannotBeRewritten(
        GovernedDecisionStateStatus status)
    {
        var store = Store();
        var first = await ActivateFirst(store);
        var request = new LifecycleTransitionRequest(
            "terminal", Address(), first.StateId!, first.Generation!.Value, status, "End authority.");
        var commit = new LifecycleTransitionCommit(request, LifecycleTestData.Actor);
        var result = await store.CommitTransitionAsync(commit, CancellationToken.None);
        var replay = await store.CommitTransitionAsync(commit, CancellationToken.None);
        var different = await store.CommitTransitionAsync(
            commit with { Request = request with { TransitionId = "different" } },
            CancellationToken.None);
        var runtime = new GovernedStateRuntimeProjection(store, "app", "dev");

        Assert.Equal(LifecycleMutationStatus.Applied, result.Status);
        Assert.Equal(LifecycleJson.Digest(result), LifecycleJson.Digest(replay));
        Assert.Equal(LifecycleMutationStatus.Rejected, different.Status);
        Assert.Contains("invalid-lifecycle-transition", different.Reasons);
        Assert.Equal(status,
            (await store.GetStateAsync(first.StateId!, CancellationToken.None))!.LifecycleStatus);
        Assert.Null(await runtime.GetActiveAsync(
            "decision", "definition", "revision", [LifecycleTestData.Target], CancellationToken.None));
    }

    [Fact]
    public async Task ActivationAfterCompletionContinuesTheAuditedGenerationChain()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        await store.CommitTransitionAsync(
            new LifecycleTransitionCommit(
                new LifecycleTransitionRequest(
                    "complete", Address(), first.StateId!, first.Generation!.Value,
                    GovernedDecisionStateStatus.Completed, "Complete."),
                LifecycleTestData.Actor),
            CancellationToken.None);
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("next", 850, new(first.StateId, first.Generation!.Value)),
            "next");
        var next = await LifecycleTestData.ActivateAsync(store, "next", "next");

        Assert.Equal(LifecycleMutationStatus.Applied, next.Status);
        Assert.Equal(2, next.Generation);
        Assert.Equal(first.StateId, next.PredecessorStateId);
        Assert.Equal(GovernedDecisionStateStatus.Completed,
            (await store.GetStateAsync(first.StateId!, CancellationToken.None))!.LifecycleStatus);
    }

    [Fact]
    public async Task RollbackLinksAndAuditsTheReplacementWithoutReactivatingHistory()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        await LifecycleTestData.ReviewAsync(
            store, LifecycleTestData.Proposal("rollback", 750, new(first.StateId, first.Generation!.Value)),
            "rollback", replacement: LifecycleReplacementKind.Rollback);
        var replacement = await LifecycleTestData.ActivateAsync(store, "rollback", "rollback");
        var replay = await LifecycleTestData.ActivateAsync(store);

        Assert.Equal(2, replacement.Generation);
        Assert.Equal(first.StateId, replacement.PredecessorStateId);
        Assert.Equal(GovernedDecisionStateStatus.RolledBack,
            (await store.GetStateAsync(first.StateId!, CancellationToken.None))!.LifecycleStatus);
        Assert.Equal(LifecycleJson.Digest(first), LifecycleJson.Digest(replay));
        Assert.Contains((await store.ReadAsync("app", "dev", CancellationToken.None)).Records,
            record => record.Kind == LifecycleAuditKind.Rollback &&
                record.PredecessorStateId == first.StateId && record.StateId == replacement.StateId);
    }

    [Fact]
    public async Task TerminalOperationIdentityCannotBeReusedWithDifferentContent()
    {
        var store = Store();
        var first = await ActivateFirst(store);
        var request = new LifecycleTransitionRequest(
            "terminal", Address(), first.StateId!, first.Generation!.Value,
            GovernedDecisionStateStatus.Completed, "Complete.");
        await store.CommitTransitionAsync(
            new LifecycleTransitionCommit(request, LifecycleTestData.Actor), CancellationToken.None);
        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(() =>
            store.CommitTransitionAsync(
                new LifecycleTransitionCommit(request with { Reason = "Changed." }, LifecycleTestData.Actor),
                CancellationToken.None));
        Assert.Equal("transition-conflict", error.Code);
    }

    private static InMemoryGovernedStateLifecycleStore Store() => new(new TestClock());

    private static async Task<LifecycleActivationReceipt> ActivateFirst(IGovernedStateLifecycleStore store)
    {
        await LifecycleTestData.ReviewAsync(store);
        return await LifecycleTestData.ActivateAsync(store);
    }

    private static GovernedStateAddress Address() => new("app", "dev", "decision", LifecycleTestData.Target);

    private sealed record UnsupportedProposal(DecisionProposalContext Context) : DecisionProposal(Context);

    private sealed class TestClock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }
}
