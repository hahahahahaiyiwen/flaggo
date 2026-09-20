using System.Text.Json;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

internal static class LifecycleTestData
{
    public static readonly DateTimeOffset Now =
        new(2026, 9, 20, 20, 0, 0, TimeSpan.Zero);

    public static readonly DecisionTargetRef Target = new("cohort", "canary");

    public static readonly GovernedDefinitionIdentity Definition = new(
        "app",
        "dev",
        "decision",
        new RuntimeContractIdentity(
            "definition",
            $"sha256:{new string('a', 64)}",
            "revision"));

    public static LifecycleActor Actor => new(
        "operator",
        "test-issuer",
        "app",
        "dev",
        CanReview: true,
        CanActivate: true);

    public static FixedValueDecisionProposal Proposal(
        string id = "proposal-1",
        double value = 800,
        GovernedStateBaseline? baseline = null) =>
        new(
            new DecisionProposalContext(
                id,
                new DecisionProposalSource(
                    DecisionProposalSourceKind.Scripted,
                    "test-producer"),
                Definition,
                Target,
                baseline ?? new GovernedStateBaseline(null, 0),
                "Improve the governed value.",
                [],
                [],
                Now.AddMinutes(-1),
                Now.AddHours(1)),
            JsonSerializer.SerializeToElement(value));

    public static RuntimeDecisionDefinition Runtime(
        DecisionPolicyContract? policy = null) =>
        new(
            "app",
            "dev",
            "decision",
            Definition.Contract,
            "number",
            JsonSerializer.SerializeToElement(800),
            "fallback",
            [new RegisteredSignalInput("pressure", "number", 0, 1)],
            [],
            NumberActionSpace: new NumberActionSpaceContract(200, 1500, 50),
            Policy: policy ?? new DecisionPolicyContract(MaximumDelta: 50),
            TargetHierarchy: ["cohort", "global"],
            InferenceTarget: "cohort",
            FallbackOrder: []);

    public static IntelligenceLifecycleDefinitionSnapshot Intelligence(
        bool strategy = false,
        DecisionPolicyContract? safety = null) =>
        new(
            "app",
            "dev",
            "decision",
            Definition.Contract,
            "active",
            new DecisionObjectives(),
            new RegisteredDecisionSignalRoles(["pressure"], [], []),
            new DecisionWorkflowPermissions(
                strategy ? "approved-strategy" : "active-value",
                strategy ? ["pressure"] : []),
            new DecisionActionSpaceContract(
                "number",
                JsonSerializer.SerializeToElement(800),
                200,
                1500,
                50),
            safety ?? new DecisionPolicyContract(MaximumDelta: 50));

    public static LifecyclePolicyLayer Layer(string revision = "policy-v1") =>
        new(
            revision,
            ["cohort", "global"],
            AutomaticApprovalAllowed: true,
            AllowSupersession: true,
            AllowRollback: true);

    public static LifecyclePolicyContext Policy => new(
        "app",
        "dev",
        "decision",
        Layer("environment-v1"),
        Layer("operator-v1"));

    public static LifecyclePolicyEvaluationRequest Evaluation(
        DecisionProposal? proposal = null,
        LifecyclePolicyContext? policy = null,
        GovernedDecisionState? baseline = null) =>
        new(
            proposal ?? Proposal(),
            LifecycleReplacementKind.Supersede,
            Actor,
            Runtime(),
            Intelligence(),
            policy ?? Policy,
            [],
            baseline,
            Now);

    public static async Task<LifecycleReviewReceipt> ReviewAsync(
        IGovernedStateLifecycleStore store,
        DecisionProposal? proposal = null,
        string reviewId = "review-1",
        LifecyclePolicyContext? policy = null,
        LifecycleReplacementKind replacement = LifecycleReplacementKind.Supersede,
        LifecycleActor? actor = null)
    {
        proposal ??= Proposal();
        var context = proposal.Context;
        var baseline = await store.GetBaselineAsync(
            new GovernedStateAddress(
                context.Definition.AppId,
                context.Definition.Environment,
                context.Definition.DecisionKey,
                context.ControlTarget),
            CancellationToken.None);
        var input = Evaluation(proposal, policy, baseline) with
        {
            Replacement = replacement,
            Actor = actor ?? Actor,
            Runtime = Runtime() with { Identity = context.Definition.Contract },
            Intelligence = Intelligence(proposal is NumericStrategyDecisionProposal) with
            {
                Identity = context.Definition.Contract
            }
        };
        var decision = await new DefaultLifecyclePolicyEvaluator()
            .EvaluateAsync(input, CancellationToken.None);
        return await store.CommitReviewAsync(
            new LifecycleReviewCommit(
                new LifecycleReviewRequest(reviewId, proposal, replacement),
                input.Actor,
                decision,
                LifecycleJson.Digest(new { input.Runtime, input.Intelligence }),
                input.Policy,
                input.Evidence,
                baseline),
            CancellationToken.None);
    }

    public static async Task<LifecycleActivationReceipt> ActivateAsync(
        IGovernedStateLifecycleStore store,
        string reviewId = "review-1",
        string activationId = "activation-1")
    {
        var review = await store.GetReviewAsync(reviewId, CancellationToken.None)
            ?? throw new InvalidOperationException("The test review is missing.");
        return await store.CommitActivationAsync(
            new LifecycleActivationCommit(
                new LifecycleActivationRequest(activationId, reviewId),
                Actor,
                LifecycleIdentity.InputsDigest(review.Commit),
                review.Commit.Decision),
            CancellationToken.None);
    }
}
