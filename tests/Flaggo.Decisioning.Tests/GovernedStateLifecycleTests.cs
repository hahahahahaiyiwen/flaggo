using System.Text.Json;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class GovernedStateLifecycleTests
{
    private static readonly DateTimeOffset Now =
        new(2026, 8, 21, 20, 0, 0, TimeSpan.Zero);

    [Fact]
    public async Task ActivateAsync_ActiveValuePublishesThroughReadOnlyProjection()
    {
        var store = Store("state-1");
        var state = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
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
        Assert.Equal("approval-activation-1", state.ApprovalReference);
        Assert.Equal(Now, state.ActivatedAt);
        Assert.Equal(Now, state.LastChangedAt);
        Assert.Equal(GovernedDecisionStateStatus.Active, state.LifecycleStatus);
        Assert.Equal("active-value", state.Mode);
        Assert.Null(state.StrategyId);
        Assert.Null(state.NumericRule);
        Assert.Equal(800, state.Value.GetInt32());
    }

    [Fact]
    public async Task ActivateAsync_NumericRuleDerivesStableStrategyIdentity()
    {
        var first = Store("state-1");
        var second = Store("state-2");
        var request = NumericRuleRequest(
            "activation-1",
            "proposal-1",
            EmptyBaseline());

        var firstState = await first.ActivateAsync(
            request,
            CancellationToken.None);
        var secondState = await second.ActivateAsync(
            request,
            CancellationToken.None);

        Assert.Equal("numeric-rule", firstState.Mode);
        Assert.StartsWith("strategy_", firstState.StrategyId);
        Assert.Equal(firstState.StrategyId, secondState.StrategyId);
        Assert.Equal(800, firstState.Value.GetInt32());
        Assert.Equal("pressure", firstState.NumericRule!.InputSignalKey);
    }

    [Fact]
    public async Task ActivateAsync_ReplayReturnsOriginalStateAndStrategyIdentity()
    {
        var store = Store("state-1", "state-unexpected");
        var request = NumericRuleRequest(
            "activation-1",
            "proposal-1",
            EmptyBaseline());

        var first = await store.ActivateAsync(
            request,
            CancellationToken.None);
        var replay = await store.ActivateAsync(
            request,
            CancellationToken.None);

        Assert.Same(first, replay);
        Assert.Equal("state-1", replay.StateId);
        Assert.Equal(first.Generation, replay.Generation);
        Assert.Equal(first.StrategyId, replay.StrategyId);
    }

    [Fact]
    public async Task ActivateAsync_ReplayAfterReplacementReturnsOriginalLineage()
    {
        var store = Store("state-1", "state-2");
        var originalRequest = NumericRuleRequest(
            "activation-1",
            "proposal-1",
            EmptyBaseline());
        var original = await store.ActivateAsync(
            originalRequest,
            CancellationToken.None);
        var replacement = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-2",
                "proposal-2",
                new GovernedStateBaseline(
                    original.StateId,
                    original.Generation),
                850),
            CancellationToken.None);

        var replay = await store.ActivateAsync(
            originalRequest,
            CancellationToken.None);

        Assert.Equal(original.StateId, replay.StateId);
        Assert.Equal(original.Generation, replay.Generation);
        Assert.Equal(original.StrategyId, replay.StrategyId);
        Assert.Equal(
            GovernedDecisionStateStatus.Superseded,
            replay.LifecycleStatus);
        Assert.NotEqual(replacement.StateId, replay.StateId);
    }

    [Fact]
    public async Task ActivateAsync_ReusedActivationWithChangedRequestConflicts()
    {
        var store = Store("state-1");
        await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-1",
                    "proposal-1",
                    EmptyBaseline(),
                    850),
                CancellationToken.None));

        Assert.Equal("activation-conflict", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_ReusedProposalWithDifferentActivationConflicts()
    {
        var store = Store("state-1");
        await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-2",
                    "proposal-1",
                    EmptyBaseline(),
                    800),
                CancellationToken.None));

        Assert.Equal("duplicate-proposal", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_StaleBaselineCannotOverwriteNewerAuthority()
    {
        var store = Store("state-1", "state-2");
        var first = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);
        var stale = new GovernedStateBaseline(
            first.StateId,
            first.Generation);
        await store.ActivateAsync(
            ActiveValueRequest(
                "activation-2",
                "proposal-2",
                stale,
                850),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-3",
                    "proposal-3",
                    stale,
                    750),
                CancellationToken.None));

        Assert.Equal("stale-baseline", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_NoStateBaselineConflictsWhenAuthorityExists()
    {
        var store = Store("state-1");
        await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-2",
                    "proposal-2",
                    EmptyBaseline(),
                    850),
                CancellationToken.None));

        Assert.Equal("stale-baseline", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_BaselineFromAnotherTargetConflicts()
    {
        var store = Store("state-1");
        var first = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800,
                new DecisionTargetRef("cohort", "first")),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-2",
                    "proposal-2",
                    new GovernedStateBaseline(
                        first.StateId,
                        first.Generation),
                    850,
                    new DecisionTargetRef("cohort", "second")),
                CancellationToken.None));

        Assert.Equal("target-conflict", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_SameRevisionWithChangedDigestConflicts()
    {
        var store = Store("state-1");
        var first = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<GovernedStateConflictException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-2",
                    "proposal-2",
                    new GovernedStateBaseline(
                        first.StateId,
                        first.Generation),
                    850,
                    contractDigest: Digest('b')),
                CancellationToken.None));

        Assert.Equal("incompatible-definition", error.Code);
    }

    [Fact]
    public async Task ActivateAsync_ConcurrentCandidatesPublishExactlyOneReplacement()
    {
        var store = Store("state-1", "state-2", "state-3");
        var first = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);
        var baseline = new GovernedStateBaseline(
            first.StateId,
            first.Generation);
        var requests = new[]
        {
            ActiveValueRequest(
                "activation-2",
                "proposal-2",
                baseline,
                850),
            ActiveValueRequest(
                "activation-3",
                "proposal-3",
                baseline,
                750)
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
        Assert.Equal(
            2,
            (await store.GetBaselineAsync(
                Address(),
                CancellationToken.None))!.Generation);
    }

    [Fact]
    public async Task ActivateAsync_ReplacementSupersedesPredecessor()
    {
        var store = Store("state-1", "state-2");
        var first = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                800),
            CancellationToken.None);
        var replacement = await store.ActivateAsync(
            ActiveValueRequest(
                "activation-2",
                "proposal-2",
                new GovernedStateBaseline(
                    first.StateId,
                    first.Generation),
                850),
            CancellationToken.None);

        var snapshot = store.CapturePersistenceSnapshot();
        var predecessor = Assert.Single(
            snapshot.States,
            entry => entry.State.StateId == first.StateId);
        Assert.Equal(
            GovernedDecisionStateStatus.Superseded,
            predecessor.State.LifecycleStatus);
        Assert.Equal(first.StateId, replacement.PredecessorStateId);
        Assert.Equal(
            GovernedDecisionStateStatus.Active,
            replacement.LifecycleStatus);
    }

    [Fact]
    public async Task ActivateAsync_UnsupportedCandidateFailsBeforePublication()
    {
        var store = Store("state-unexpected");

        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(
            () => store.ActivateAsync(
                Request(
                    "activation-1",
                    "proposal-1",
                    EmptyBaseline(),
                    new UnsupportedCandidate("unsupported")),
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
                Request(
                    "activation-1",
                    "proposal-1",
                    EmptyBaseline(),
                    new ActiveValueActivationCandidate(
                        "Improve the governed value.",
                        JsonSerializer.SerializeToElement(
                            new { unsupported = true }))),
                CancellationToken.None));

        Assert.Equal("invalid-activation", error.Code);
        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    [Theory]
    [InlineData("empty-inputs")]
    [InlineData("threshold-below-range")]
    [InlineData("threshold-above-range")]
    public async Task ActivateAsync_InvalidWeightedRuleFailsBeforePublication(
        string invalidRule)
    {
        var store = Store("state-unexpected");
        var threshold = invalidRule switch
        {
            "threshold-below-range" => -0.1,
            "threshold-above-range" => 1.1,
            _ => 0.5
        };
        IReadOnlyList<NumericRuleInput> inputs = invalidRule == "empty-inputs"
            ? []
            : [new NumericRuleInput("pressure", 0, 1, 1)];

        var error = await Assert.ThrowsAsync<GovernedStateValidationException>(
            () => store.ActivateAsync(
                Request(
                    "activation-1",
                    "proposal-1",
                    EmptyBaseline(),
                    new NumericRuleActivationCandidate(
                        "Adapt the governed value.",
                        JsonSerializer.SerializeToElement(800),
                        new NumericRuleStrategy(
                            "pressure",
                            threshold,
                            750,
                            850,
                            inputs))),
                CancellationToken.None));

        Assert.Equal("invalid-activation", error.Code);
        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    [Fact]
    public async Task ActivateAsync_FreezesWeightedRuleAuthority()
    {
        var store = Store("state-1");
        var inputs = new[]
        {
            new NumericRuleInput("pressure", 0, 1, 1)
        };
        var activated = await store.ActivateAsync(
            Request(
                "activation-1",
                "proposal-1",
                EmptyBaseline(),
                new NumericRuleActivationCandidate(
                    "Adapt the governed value.",
                    JsonSerializer.SerializeToElement(800),
                    new NumericRuleStrategy(
                        "pressure",
                        0.5,
                        750,
                        850,
                        inputs))),
            CancellationToken.None);

        inputs[0] = new NumericRuleInput("tampered", 0, 1, 1);
        var baseline = await store.GetBaselineAsync(
            Address(),
            CancellationToken.None);
        var storedInputs = baseline!.NumericRule!.WeightedInputs!;

        Assert.Equal("pressure", Assert.Single(storedInputs).SignalKey);
        var exposedList =
            Assert.IsAssignableFrom<IList<NumericRuleInput>>(storedInputs);
        Assert.Throws<NotSupportedException>(
            () => exposedList[0] =
                new NumericRuleInput("tampered-again", 0, 1, 1));
        Assert.Equal(activated.StrategyId, baseline.StrategyId);
    }

    [Fact]
    public async Task ActivateAsync_CancellationLeavesAuthorityUnchanged()
    {
        var store = Store("state-unexpected");
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => store.ActivateAsync(
                ActiveValueRequest(
                    "activation-1",
                    "proposal-1",
                    EmptyBaseline(),
                    800),
                cancellation.Token));

        Assert.Null(await store.GetBaselineAsync(
            Address(),
            CancellationToken.None));
    }

    private static InMemoryGovernedStateLifecycleStore Store(
        params string[] stateIds) =>
        new(
            new FixedTimeProvider(),
            new SequenceStateIdentityGenerator(stateIds));

    private static GovernedStateActivationRequest ActiveValueRequest(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline,
        int value,
        DecisionTargetRef? target = null,
        string? contractDigest = null) =>
        Request(
            activationId,
            proposalId,
            baseline,
            new ActiveValueActivationCandidate(
                "Improve the governed value.",
                JsonSerializer.SerializeToElement(value)),
            target,
            contractDigest);

    private static GovernedStateActivationRequest NumericRuleRequest(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline) =>
        Request(
            activationId,
            proposalId,
            baseline,
            new NumericRuleActivationCandidate(
                "Adapt the governed value.",
                JsonSerializer.SerializeToElement(800),
                new NumericRuleStrategy("pressure", 0.5, 750, 850)));

    private static GovernedStateActivationRequest Request(
        string activationId,
        string proposalId,
        GovernedStateBaseline baseline,
        GovernedStateActivationCandidate candidate,
        DecisionTargetRef? target = null,
        string? contractDigest = null) =>
        new(
            activationId,
            proposalId,
            $"approval-{activationId}",
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
            candidate);

    private static GovernedStateAddress Address(
        DecisionTargetRef? target = null) =>
        new("app", "dev", "decision", target);

    private static GovernedStateBaseline EmptyBaseline() => new(null, 0);

    private static string Digest(char value) => $"sha256:{new string(value, 64)}";

    private sealed record UnsupportedCandidate(string Rationale)
        : GovernedStateActivationCandidate(Rationale);

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
