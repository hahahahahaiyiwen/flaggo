using System.Text.Json;
using Flaggo.Decisioning;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class DecisionPortTests
{
    [Fact]
    public async Task NumericRuleExecutor_DerivesCandidateFromRuntimeInput()
    {
        var executor = new DeterministicStrategyExecutor();
        var state = State() with
        {
            Mode = "strategy",
            StrategyId = "strategy-test",
            NumericRule = new NumericRuleStrategy(
                "tetris.boardPressure",
                0.75,
                700,
                800)
        };
        var evidence = Evidence(0.82);

        var result = await executor.ExecuteAsync(
            new StrategyExecutionRequest(
                state,
                [new SignalInput(
                    new SignalRef("tetris.boardPressure"),
                    JsonSerializer.SerializeToElement(0.82))],
                evidence),
            CancellationToken.None);

        Assert.Equal(700, result.Candidate!.Value.GetInt32());
        Assert.Equal("strategy", result.Mode);
        Assert.Equal(0.82, result.Confidence!.EvidenceQuality);
    }

    [Theory]
    [InlineData(0.9, 1600, 3, 8, 850)]
    [InlineData(0.2, 400, 0, 10, 750)]
    public async Task NumericRuleExecutor_UsesAllWeightedTetrisInputs(
        double boardPressure,
        double placementTime,
        double recoveryFailures,
        double currentLevel,
        int expected)
    {
        var executor = new DeterministicStrategyExecutor();
        var state = State() with
        {
            Mode = "strategy",
            StrategyId = "strategy-tetris-balanced-v1",
            NumericRule = TetrisRule()
        };

        var result = await executor.ExecuteAsync(
            new StrategyExecutionRequest(
                state,
                [
                    Input("tetris.boardPressure", boardPressure),
                    Input("tetris.recentPlacementTimeMs", placementTime),
                    Input("tetris.recoveryFailures", recoveryFailures),
                    Input("tetris.currentLevel", currentLevel)
                ],
                Evidence(0.82)),
            CancellationToken.None);

        Assert.Equal(expected, result.Candidate!.Value.GetInt32());
        Assert.Null(result.FailureReason);
        Assert.NotNull(result.Confidence);
    }

    [Fact]
    public async Task NumericRuleExecutor_FailsClosedWithoutConfidenceEvidence()
    {
        var executor = new DeterministicStrategyExecutor();
        var state = State() with
        {
            Mode = "strategy",
            StrategyId = "strategy-tetris-balanced-v1",
            NumericRule = TetrisRule()
        };

        var result = await executor.ExecuteAsync(
            new StrategyExecutionRequest(
                state,
                [
                    Input("tetris.boardPressure", 0.9),
                    Input("tetris.recentPlacementTimeMs", 1600),
                    Input("tetris.recoveryFailures", 3),
                    Input("tetris.currentLevel", 8)
                ],
                null),
            CancellationToken.None);

        Assert.Null(result.Candidate);
        Assert.Null(result.Confidence);
        Assert.Equal("strategy_confidence_unavailable", result.FailureReason);
    }

    [Fact]
    public async Task NumericRuleExecutor_FailsClosedWhenWeightedInputIsMissing()
    {
        var executor = new DeterministicStrategyExecutor();
        var state = State() with
        {
            Mode = "strategy",
            StrategyId = "strategy-tetris-balanced-v1",
            NumericRule = TetrisRule()
        };

        var result = await executor.ExecuteAsync(
            new StrategyExecutionRequest(
                state,
                [
                    Input("tetris.boardPressure", 0.9),
                    Input("tetris.recentPlacementTimeMs", 1600),
                    Input("tetris.recoveryFailures", 3)
                ],
                null),
            CancellationToken.None);

        Assert.Null(result.Candidate);
        Assert.Equal("invalid_strategy_input", result.FailureReason);
    }

    [Fact]
    public async Task PolicyEvaluator_ApprovesBoundedCandidate()
    {
        var evaluator = new DefaultPolicyEvaluator(new FixedTimeProvider());

        var result = await evaluator.EvaluateAsync(
            new PolicyEvaluationRequest(
                JsonSerializer.SerializeToElement(700),
                JsonSerializer.SerializeToElement(750),
                new NumberActionSpaceContract(200, 1500, 50),
                new DecisionPolicyContract(
                    Minimum: 200,
                    Maximum: 1500,
                    MaximumDelta: 50,
                    CooldownSeconds: 20,
                    MinimumEvidenceQuality: 0.7,
                    MaximumModelUncertainty: 0.35,
                    MinimumSampleSize: 30),
                Evidence(0.82),
                new DateTimeOffset(2026, 7, 31, 17, 59, 0, TimeSpan.Zero),
                null),
            CancellationToken.None);

        Assert.True(result.Approved);
        Assert.Equal("approved", result.Result.Result);
        Assert.Contains("number-bounds", result.Result.AppliedConstraints);
        Assert.Contains("max-delta", result.Result.AppliedConstraints);
        Assert.Contains("cooldown", result.Result.AppliedConstraints);
    }

    [Fact]
    public async Task PolicyEvaluator_BlocksUnsafeCandidateAndEvidence()
    {
        var evaluator = new DefaultPolicyEvaluator(new FixedTimeProvider());

        var result = await evaluator.EvaluateAsync(
            new PolicyEvaluationRequest(
                JsonSerializer.SerializeToElement(1750),
                JsonSerializer.SerializeToElement(800),
                new NumberActionSpaceContract(200, 1500, 50),
                new DecisionPolicyContract(
                    Minimum: 200,
                    Maximum: 1500,
                    MaximumDelta: 50,
                    MinimumEvidenceQuality: 0.7,
                    Paused: true),
                Evidence(0.5),
                null,
                null),
            CancellationToken.None);

        Assert.False(result.Approved);
        Assert.Equal("blocked", result.Result.Result);
        Assert.Contains("candidate_out_of_bounds", result.Result.Reasons);
        Assert.Contains("max_delta_exceeded", result.Result.Reasons);
        Assert.Contains("insufficient_evidence_quality", result.Result.Reasons);
        Assert.Contains("decision_paused", result.Result.Reasons);
    }

    [Fact]
    public async Task PolicyEvaluator_AcceptsHugeFiniteCooldownWithoutOverflow()
    {
        var result = await EvaluateCooldownAsync(
            DateTimeOffset.MaxValue,
            DateTimeOffset.MinValue,
            double.MaxValue);

        Assert.False(result.Approved);
        Assert.Contains("cooldown_active", result.Result.Reasons);
        Assert.DoesNotContain("invalid_cooldown", result.Result.Reasons);
    }

    [Fact]
    public async Task PolicyEvaluator_HandlesMinimumAndMaximumTimestamps()
    {
        var atMaximum = await EvaluateCooldownAsync(
            DateTimeOffset.MaxValue,
            DateTimeOffset.MaxValue,
            1);
        var elapsedFromMinimum = await EvaluateCooldownAsync(
            DateTimeOffset.MaxValue,
            DateTimeOffset.MinValue,
            1);

        Assert.False(atMaximum.Approved);
        Assert.Contains("cooldown_active", atMaximum.Result.Reasons);
        Assert.True(elapsedFromMinimum.Approved);
    }

    [Fact]
    public async Task PolicyEvaluator_BlocksHugeValidCooldown()
    {
        var result = await EvaluateCooldownAsync(
            DateTimeOffset.MaxValue,
            DateTimeOffset.MinValue,
            double.MaxValue);

        Assert.False(result.Approved);
        Assert.Contains("cooldown_active", result.Result.Reasons);
    }

    [Fact]
    public async Task PolicyEvaluator_BlocksFutureTimestampWithZeroCooldown()
    {
        var now = new DateTimeOffset(2026, 8, 6, 0, 0, 0, TimeSpan.Zero);

        var result = await EvaluateCooldownAsync(
            now,
            now.AddTicks(1),
            0);

        Assert.False(result.Approved);
        Assert.Contains("cooldown_active", result.Result.Reasons);
    }

    [Fact]
    public async Task TargetResolver_ProducesDeterministicFallbackPlan()
    {
        var resolver = new DefaultTargetResolver(
            new Dictionary<string, string>
            {
                ["new_players"] = "new_players"
            });
        var runtimeTarget = new DecisionTargetRef("session", "game-1");
        var context = new Dictionary<string, JsonElement>
        {
            ["userId"] = JsonSerializer.SerializeToElement("user-1"),
            ["cohort"] = JsonSerializer.SerializeToElement("new_players")
        };

        var plan = await resolver.ResolveAsync(
            runtimeTarget,
            context,
            CancellationToken.None);
        var result = plan.Describe(
            new DecisionTargetRef("cohort", "new_players"),
            selectedTargetIndex: 2);

        Assert.Equal("session", plan.StateTargets[0]!.Type);
        Assert.Equal("user", plan.StateTargets[1]!.Type);
        Assert.Equal("cohort", plan.StateTargets[2]!.Type);
        Assert.Equal(
            ["session:game-1", "user:user-1", "cohort:new_players", "global"],
            plan.ResolutionChain);
        Assert.Equal(
            "client-verified",
            Assert.Single(result.TargetProvenance).Source);
    }

    [Fact]
    public async Task TargetResolver_AttributesGlobalFallbackToClaimedCohort()
    {
        var resolver = new DefaultTargetResolver(
            new Dictionary<string, string>
            {
                ["new_players"] = "new_players"
            });
        var context = new Dictionary<string, JsonElement>
        {
            ["cohort"] = JsonSerializer.SerializeToElement("new_players")
        };
        var plan = await resolver.ResolveAsync(
            new DecisionTargetRef("session", "game-1"),
            context,
            CancellationToken.None);

        var result = plan.Describe(
            new DecisionTargetRef("global", "global"),
            selectedTargetIndex: 1);

        var provenance = Assert.Single(result.TargetProvenance);
        Assert.Equal("cohort", provenance.TargetType);
        Assert.Equal("new_players", provenance.ClaimedId);
        Assert.Equal("global", provenance.ResolvedId);
        Assert.Equal("server-derived", provenance.Source);
    }

    [Fact]
    public async Task TargetResolver_SurfacesAuthoritativeCohortReplacement()
    {
        var resolver = new DefaultTargetResolver(
            new Dictionary<string, string>
            {
                ["whales"] = "new_players"
            });
        var context = new Dictionary<string, JsonElement>
        {
            ["cohort"] = JsonSerializer.SerializeToElement("whales")
        };

        var plan = await resolver.ResolveAsync(null, context, CancellationToken.None);
        var result = plan.Describe(plan.StateTargets[0], selectedTargetIndex: 0);
        var provenance = Assert.Single(result.TargetProvenance);

        Assert.Equal("new_players", plan.StateTargets[0]!.Id);
        Assert.Equal("server-replaced", provenance.Source);
        Assert.Equal("whales", provenance.ClaimedId);
        Assert.Equal("new_players", provenance.ResolvedId);
        Assert.Contains("cohort:new_players", plan.ResolutionChain);
    }

    [Fact]
    public async Task TargetResolver_DoesNotTrustUnverifiedCohortClaim()
    {
        var resolver = new DefaultTargetResolver();
        var context = new Dictionary<string, JsonElement>
        {
            ["cohort"] = JsonSerializer.SerializeToElement("admin_users")
        };

        var plan = await resolver.ResolveAsync(null, context, CancellationToken.None);

        Assert.DoesNotContain(
            plan.StateTargets,
            target => target?.Type == "cohort");
        Assert.DoesNotContain("cohort:admin_users", plan.ResolutionChain);
    }

    [Fact]
    public async Task TargetResolver_DoesNotTrustUnverifiedCohortRuntimeTarget()
    {
        var resolver = new DefaultTargetResolver();
        var claimedTarget = new DecisionTargetRef("cohort", "admin_users");

        var plan = await resolver.ResolveAsync(
            claimedTarget,
            new Dictionary<string, JsonElement>(),
            CancellationToken.None);

        Assert.DoesNotContain(
            plan.StateTargets,
            target => target?.Type == "cohort");
        Assert.DoesNotContain("cohort:admin_users", plan.ResolutionChain);
    }

    [Fact]
    public async Task TargetResolver_PreservesMappedRuntimeCohortProvenance()
    {
        var resolver = new DefaultTargetResolver(
            new Dictionary<string, string>
            {
                ["whales"] = "new_players"
            });

        var plan = await resolver.ResolveAsync(
            new DecisionTargetRef("cohort", "whales"),
            new Dictionary<string, JsonElement>(),
            CancellationToken.None);
        var result = plan.Describe(plan.StateTargets[0], selectedTargetIndex: 0);
        var provenance = Assert.Single(result.TargetProvenance);

        Assert.Equal("whales", provenance.ClaimedId);
        Assert.Equal("new_players", provenance.ResolvedId);
        Assert.Equal("server-replaced", provenance.Source);
    }

    private static GovernedDecisionState State() =>
        new(
            "def-test",
            "rev-test",
            $"sha256:{new string('a', 64)}",
            JsonSerializer.SerializeToElement(800));

    private static NumericRuleStrategy TetrisRule() => new(
        "tetris.boardPressure",
        0.55,
        850,
        750,
        [
            new NumericRuleInput("tetris.boardPressure", 0, 1, 0.45),
            new NumericRuleInput("tetris.recentPlacementTimeMs", 0, 2000, 0.25),
            new NumericRuleInput("tetris.recoveryFailures", 0, 5, 0.20),
            new NumericRuleInput("tetris.currentLevel", 0, 20, 0.10)
        ]);

    private static SignalInput Input(string key, double value) => new(
        new SignalRef(key),
        JsonSerializer.SerializeToElement(value));

    private static DecisionEvidenceSnapshot Evidence(double quality) =>
        new(
            quality,
            ModelUncertainty: 0.31,
            ExpectedOutcome: 0.72,
            SampleSize: 50,
            Details: new Dictionary<string, JsonElement>
            {
                ["qualitySource"] = JsonSerializer.SerializeToElement("test")
            });

    private static Task<PolicyDecision> EvaluateCooldownAsync(
        DateTimeOffset now,
        DateTimeOffset changedAt,
        double cooldown) =>
        new DefaultPolicyEvaluator(new FixedTimeProvider(now)).EvaluateAsync(
            new PolicyEvaluationRequest(
                JsonSerializer.SerializeToElement(800),
                JsonSerializer.SerializeToElement(800),
                null,
                new DecisionPolicyContract(CooldownSeconds: cooldown),
                null,
                changedAt,
                null),
            CancellationToken.None);

    private sealed class FixedTimeProvider(DateTimeOffset? utcNow = null) :
        TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            utcNow ??
            new DateTimeOffset(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);
    }
}
