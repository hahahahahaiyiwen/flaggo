using System.Text.Json;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class GovernedStateLifecycleTests
{
    private static readonly DateTimeOffset Now =
        new(2026, 8, 21, 20, 0, 0, TimeSpan.Zero);

    [Fact]
    public async Task ActivateAsync_FixedProposalPublishesCompleteStateThroughReadOnlyProjection()
    {
        var store = Store("state-1");
        var proposal = FixedProposal("proposal-1", EmptyBaseline(), 800);

        var state = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                proposal),
            CancellationToken.None);
        var runtime = new GovernedStateRuntimeProjection(store, "app", "dev");
        var projected = await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None);

        Assert.Same(state, projected);
        Assert.Equal("state-1", state.StateId);
        Assert.Equal("proposal-1", state.ProposalId);
        Assert.Equal(1, state.Generation);
        Assert.Null(state.PredecessorStateId);
        Assert.Equal("approval-1", state.ApprovalReference);
        Assert.Equal(Now, state.ActivatedAt);
        Assert.Equal(Now, state.LastChangedAt);
        Assert.Equal(GovernedDecisionStateStatus.Active, state.LifecycleStatus);
        Assert.Equal("active-value", state.Mode);
        Assert.Equal(800, state.Value.GetInt32());
    }

    [Fact]
    public async Task ActivateAsync_NumericStrategyCreatesSupportedStrategyState()
    {
        var store = Store("state-1");
        var proposal = StrategyProposal("proposal-1", EmptyBaseline());

        var state = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                proposal),
            CancellationToken.None);

        Assert.Equal("strategy", state.Mode);
        Assert.Equal("strategy-1", state.StrategyId);
        Assert.Equal(800, state.Value.GetInt32());
        Assert.Equal("pressure", state.NumericRule!.InputSignalKey);
    }

    [Fact]
    public async Task ActivateAsync_ReplayReturnsOriginalStateIdentity()
    {
        var store = Store("state-1", "state-unexpected");
        var request = new GovernedStateActivationRequest(
            "activation-1",
            "approval-1",
            FixedProposal("proposal-1", EmptyBaseline(), 800));

        var first = await store.ActivateAsync(request, CancellationToken.None);
        var replay = await store.ActivateAsync(request, CancellationToken.None);

        Assert.Equal("state-1", first.StateId);
        Assert.Same(first, replay);
    }

    [Fact]
    public async Task ActivateAsync_ReusedActivationWithChangedRequestConflicts()
    {
        var store = Store("state-1");
        await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-1",
                    "approval-2",
                    FixedProposal("proposal-1", EmptyBaseline(), 800)),
                CancellationToken.None));

        Assert.Equal("activation-conflict", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_ReusedProposalWithDifferentActivationConflicts()
    {
        var store = Store("state-1");
        var proposal = FixedProposal("proposal-1", EmptyBaseline(), 800);
        await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                proposal),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-2",
                    "approval-1",
                    proposal),
                CancellationToken.None));

        Assert.Equal("duplicate-proposal", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_StaleBaselineCannotOverwriteNewerAuthority()
    {
        var store = Store("state-1", "state-2");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var stale = new GovernedStateBaseline(first.StateId, first.Generation);
        await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-2",
                "approval-2",
                FixedProposal("proposal-2", stale, 850)),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-3",
                    "approval-3",
                    FixedProposal("proposal-3", stale, 750)),
                CancellationToken.None));

        Assert.Equal("stale-baseline", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_BaselineFromAnotherTargetConflicts()
    {
        var store = Store("state-1");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal(
                    "proposal-1",
                    EmptyBaseline(),
                    800,
                    new DecisionTargetRef("cohort", "first"))),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-2",
                    "approval-2",
                    FixedProposal(
                        "proposal-2",
                        new GovernedStateBaseline(
                            first.StateId,
                            first.Generation),
                        850,
                        new DecisionTargetRef("cohort", "second"))),
                CancellationToken.None));

        Assert.Equal("target-conflict", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_SameRevisionWithChangedDigestConflicts()
    {
        var store = Store("state-1");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-2",
                    "approval-2",
                    FixedProposal(
                        "proposal-2",
                        new GovernedStateBaseline(
                            first.StateId,
                            first.Generation),
                        850,
                        contractDigest: Digest('b'))),
                CancellationToken.None));

        Assert.Equal("incompatible-definition", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_ConcurrentCandidatesPublishExactlyOneReplacement()
    {
        var store = Store("state-1", "state-2", "state-3");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var baseline = new GovernedStateBaseline(
            first.StateId,
            first.Generation);
        var requests = new[]
        {
            new GovernedStateActivationRequest(
                "activation-2",
                "approval-2",
                FixedProposal("proposal-2", baseline, 850)),
            new GovernedStateActivationRequest(
                "activation-3",
                "approval-3",
                FixedProposal("proposal-3", baseline, 750))
        };

        var outcomes = await Task.WhenAll(
            requests.Select(async request =>
            {
                try
                {
                    return (State: await store.ActivateAsync(
                        request,
                        CancellationToken.None), Error: (Exception?)null);
                }
                catch (Exception error)
                {
                    return (State: (GovernedDecisionState?)null, Error: error);
                }
            }));

        Assert.Single(outcomes.Where(outcome => outcome.State is not null));
        var conflict = Assert.IsType<GovernedStateConflictException>(
            Assert.Single(outcomes.Where(outcome => outcome.Error is not null)).Error);
        Assert.Equal("stale-baseline", conflict.Code);
        var active = await store.GetBaselineAsync(
            Address(),
            CancellationToken.None);
        Assert.Equal(2, active!.Generation);
    }

    [Fact]
    public async Task ActivateAsync_UnsupportedProposalFailsBeforePublication()
    {
        var store = Store("state-unexpected");

        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-1",
                    "approval-1",
                    new UnsupportedProposal(
                        Context("proposal-1", EmptyBaseline(), null))),
                CancellationToken.None));

        Assert.Equal("unsupported-state-kind", error.Code);
        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    [Fact]
    public async Task ActivateAsync_MalformedValueFailsBeforePublication()
    {
        var store = Store("state-unexpected");

        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-1",
                    "approval-1",
                    new FixedValueDecisionProposal(
                        Context("proposal-1", EmptyBaseline(), null),
                        JsonSerializer.SerializeToElement(
                            new { unsupported = true }))),
                CancellationToken.None));

        Assert.Equal("invalid-proposal", error.Code);
        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    [Fact]
    public async Task ActivateAsync_CancellationLeavesAuthorityUnchanged()
    {
        var store = Store("state-unexpected");
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => store.ActivateAsync(
                new GovernedStateActivationRequest(
                    "activation-1",
                    "approval-1",
                    FixedProposal("proposal-1", EmptyBaseline(), 800)),
                cancellation.Token));

        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    [Fact]
    public async Task TransitionAsync_CompletesAuthorityAndRemovesRuntimeProjection()
    {
        var store = Store("state-1");
        var state = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var transitioned = await store.TransitionAsync(
            new GovernedStateTransitionRequest(
                "transition-1",
                Address(),
                state.StateId!,
                state.Generation,
                GovernedDecisionStateStatus.Completed),
            CancellationToken.None);
        var runtime = new GovernedStateRuntimeProjection(store, "app", "dev");

        Assert.Equal(
            GovernedDecisionStateStatus.Completed,
            transitioned.LifecycleStatus);
        var baseline = await store.GetBaselineAsync(
            Address(),
            CancellationToken.None);
        Assert.Equal(
            GovernedDecisionStateStatus.Completed,
            baseline!.LifecycleStatus);
        Assert.Null(await runtime.GetActiveAsync(
            "decision",
            "definition",
            "revision",
            [null],
            CancellationToken.None));
    }

    [Fact]
    public async Task ActivateAsync_AfterCompletionContinuesGenerationHistory()
    {
        var store = Store("state-1", "state-2");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var completed = await store.TransitionAsync(
            new GovernedStateTransitionRequest(
                "transition-1",
                Address(),
                first.StateId!,
                first.Generation,
                GovernedDecisionStateStatus.Completed),
            CancellationToken.None);

        var replacement = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-2",
                "approval-2",
                FixedProposal(
                    "proposal-2",
                    new GovernedStateBaseline(
                        completed.StateId,
                        completed.Generation),
                    850)),
            CancellationToken.None);

        Assert.Equal(2, replacement.Generation);
        Assert.Equal(first.StateId, replacement.PredecessorStateId);
        Assert.Equal(
            GovernedDecisionStateStatus.Completed,
            (await store.GetStateAsync(
                first.StateId!,
                CancellationToken.None))!.LifecycleStatus);
    }

    [Fact]
    public async Task TransitionAsync_ReplayReturnsOriginalStateIdentity()
    {
        var store = Store("state-1");
        var state = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var request = new GovernedStateTransitionRequest(
            "transition-1",
            Address(),
            state.StateId!,
            state.Generation,
            GovernedDecisionStateStatus.Completed);

        var first = await store.TransitionAsync(
            request,
            CancellationToken.None);
        var replay = await store.TransitionAsync(
            request,
            CancellationToken.None);

        Assert.Same(first, replay);
        Assert.Equal(state.StateId, replay.StateId);
    }

    [Fact]
    public async Task ActivateAsync_RollbackMarksReplacedStateRolledBack()
    {
        var store = Store("state-1", "state-2");
        var first = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-1",
                "approval-1",
                FixedProposal("proposal-1", EmptyBaseline(), 800)),
            CancellationToken.None);
        var replacement = await store.ActivateAsync(
            new GovernedStateActivationRequest(
                "activation-2",
                "approval-2",
                FixedProposal(
                    "proposal-2",
                    new GovernedStateBaseline(
                        first.StateId,
                        first.Generation),
                    750),
                GovernedDecisionStateStatus.RolledBack),
            CancellationToken.None);

        var predecessor = await store.GetStateAsync(
            first.StateId!,
            CancellationToken.None);
        Assert.Equal(
            GovernedDecisionStateStatus.RolledBack,
            predecessor!.LifecycleStatus);
        Assert.Equal(first.StateId, replacement.PredecessorStateId);
    }

    private static InMemoryGovernedStateLifecycleStore Store(
        params string[] stateIds) =>
        new(
            new FixedTimeProvider(),
            new SequenceStateIdentityGenerator(stateIds));

    private static FixedValueDecisionProposal FixedProposal(
        string proposalId,
        GovernedStateBaseline baseline,
        int value,
        DecisionTargetRef? target = null,
        string? contractDigest = null) =>
        new(
            Context(proposalId, baseline, target, contractDigest),
            JsonSerializer.SerializeToElement(value));

    private static NumericStrategyDecisionProposal StrategyProposal(
        string proposalId,
        GovernedStateBaseline baseline) =>
        new(
            Context(proposalId, baseline, null),
            JsonSerializer.SerializeToElement(800),
            "strategy-1",
            new NumericRuleStrategy("pressure", 0.5, 750, 850));

    private static DecisionProposalContext Context(
        string proposalId,
        GovernedStateBaseline baseline,
        DecisionTargetRef? target,
        string? contractDigest = null) =>
        new(
            proposalId,
            new DecisionProposalSource(
                DecisionProposalSourceKind.Scripted,
                "fixture"),
            new GovernedDefinitionIdentity(
                "app",
                "dev",
                "decision",
                new RuntimeContractIdentity(
                    "definition",
                    contractDigest ?? Digest('a'),
                    "revision")),
            target,
            baseline,
            "Improve the governed value.",
            ["evidence-1"],
            ["confidence-1"],
            Now.AddMinutes(-1),
            Now.AddHours(1));

    private static GovernedStateAddress Address(
        DecisionTargetRef? target = null) =>
        new("app", "dev", "decision", target);

    private static GovernedStateBaseline EmptyBaseline() => new(null, 0);

    private static string Digest(char value) => $"sha256:{new string(value, 64)}";

    private sealed record UnsupportedProposal(DecisionProposalContext Context)
        : DecisionProposal(Context);

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => Now;
    }

    private sealed class SequenceStateIdentityGenerator(
        IEnumerable<string> stateIds) : IGovernedStateIdentityGenerator
    {
        private readonly Queue<string> _stateIds = new(stateIds);

        public string CreateStateId() => _stateIds.Dequeue();
    }
}
