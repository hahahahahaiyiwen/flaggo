using System.Text.Json;
using Flaggo.Decisioning;
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
    public async Task TargetResolver_ProducesDeterministicFallbackPlan()
    {
        var resolver = new DefaultTargetResolver();
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
            selectedTargetIndex: 0);

        Assert.Equal("cohort", plan.StateTargets[0]!.Type);
        Assert.Equal(
            ["session:game-1", "user:user-1", "cohort:new_players", "global"],
            plan.ResolutionChain);
        Assert.Equal(
            "client-verified",
            Assert.Single(result.TargetProvenance).Source);
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

    private static GovernedDecisionState State() =>
        new(
            "def-test",
            "rev-test",
            $"sha256:{new string('a', 64)}",
            JsonSerializer.SerializeToElement(800));

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

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 7, 31, 18, 0, 0, TimeSpan.Zero);
    }
}
