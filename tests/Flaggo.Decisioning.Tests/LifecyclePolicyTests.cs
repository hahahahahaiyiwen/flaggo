using System.Text.Json;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class LifecyclePolicyTests
{
    private readonly DefaultLifecyclePolicyEvaluator _evaluator = new();

    [Theory]
    [InlineData("app")]
    [InlineData("environment")]
    [InlineData("decision")]
    [InlineData("definition")]
    [InlineData("revision")]
    [InlineData("digest")]
    public async Task SemanticDefinitionIdentityAndResourceScopeMustStillMatch(string field)
    {
        var original = LifecycleTestData.Definition;
        var definition = field switch
        {
            "app" => original with { AppId = "other" },
            "environment" => original with { Environment = "other" },
            "decision" => original with { DecisionKey = "other" },
            "definition" => original with { Contract = original.Contract with { DefinitionId = "other" } },
            "revision" => original with { Contract = original.Contract with { Revision = "other" } },
            "digest" => original with
            {
                Contract = original.Contract with { ContractDigest = $"sha256:{new string('b', 64)}" }
            },
            _ => throw new InvalidOperationException("Unknown identity field.")
        };
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with { Context = proposal.Context with { Definition = definition } };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(proposal), CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
        Assert.Contains("definition_not_authorized", result.Reasons);
    }

    [Theory]
    [InlineData(DecisionProposalSourceKind.Operator)]
    [InlineData(DecisionProposalSourceKind.Scripted)]
    [InlineData(DecisionProposalSourceKind.Automated)]
    [InlineData(DecisionProposalSourceKind.Intelligence)]
    public async Task AutomaticApprovalUsesTheSameGateForEverySource(
        DecisionProposalSourceKind source)
    {
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with
        {
            Context = proposal.Context with
            {
                Source = new DecisionProposalSource(source, "producer")
            }
        };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(proposal),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Approved, result.Disposition);
        Assert.True(result.EffectivePolicy!.AutomaticApprovalAllowed);
        var denied = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(proposal, LifecycleTestData.Policy with
            {
                OperatorControls = LifecycleTestData.Layer() with { Paused = true }
            }), CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Hold, denied.Disposition);
        Assert.Contains("lifecycle_paused", denied.Reasons);
    }

    [Fact]
    public async Task HumanRequiredCannotBeWidenedByAnAutomaticEnvironment()
    {
        var policy = LifecycleTestData.Policy with
        {
            OperatorControls = LifecycleTestData.Layer() with
            {
                AutomaticApprovalAllowed = false
            }
        };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(policy: policy),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.PendingApproval, result.Disposition);
        Assert.Contains("human_approval_required", result.Reasons);
        Assert.False(result.EffectivePolicy!.AutomaticApprovalAllowed);
    }

    [Fact]
    public async Task NarrowerBoundsReturnRestrictionsWithoutClampingTheProposal()
    {
        var proposal = LifecycleTestData.Proposal(value: 850);
        var policy = LifecycleTestData.Policy with
        {
            EnvironmentPolicy = LifecycleTestData.Layer() with { Maximum = 800 },
            OperatorControls = LifecycleTestData.Layer() with { Maximum = 1000 }
        };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(proposal, policy),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Limited, result.Disposition);
        Assert.Equal(800, result.EffectivePolicy!.Maximum);
        Assert.Equal(850, proposal.Value.GetDouble());
    }

    [Theory]
    [InlineData(175)]
    [InlineData(1550)]
    [InlineData(825)]
    public async Task DefinitionBoundsAndGridCannotBeWidened(double value)
    {
        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(
                LifecycleTestData.Proposal(value: value)),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
    }

    [Fact]
    public async Task PauseAndMissingEvidenceHoldWithoutAnApproval()
    {
        var policy = LifecycleTestData.Policy with
        {
            OperatorControls = LifecycleTestData.Layer() with
            {
                Paused = true,
                MinimumEvidenceQuality = 0.9
            }
        };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(policy: policy),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Hold, result.Disposition);
        Assert.Contains("lifecycle_paused", result.Reasons);
        Assert.Contains("required_evidence_unavailable", result.Reasons);
    }

    [Fact]
    public async Task ATargetOutsideTheIntersectionIsRejected()
    {
        var policy = LifecycleTestData.Policy with
        {
            OperatorControls = LifecycleTestData.Layer() with
            {
                AllowedTargetKinds = ["global"]
            }
        };

        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(policy: policy),
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
        Assert.Contains("target_not_authorized", result.Reasons);
    }

    [Fact]
    public async Task AStrategyMustValidateBothOutputsAndDeclaredInputs()
    {
        var fixedProposal = LifecycleTestData.Proposal();
        var proposal = new NumericStrategyDecisionProposal(
            fixedProposal.Context,
            JsonSerializer.SerializeToElement(800),
            "strategy",
            new NumericRuleStrategy("undeclared", 0.5, 850, 1600));
        var request = LifecycleTestData.Evaluation(proposal) with
        {
            Intelligence = LifecycleTestData.Intelligence(strategy: true)
        };

        var result = await _evaluator.EvaluateAsync(
            request,
            CancellationToken.None);

        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
        Assert.Contains("strategy_input_not_authorized", result.Reasons);
        Assert.Contains("candidate_out_of_bounds", result.Reasons);
    }

    [Fact]
    public async Task MissingAndConflictingPolicyCannotProduceApproval()
    {
        var missing = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation() with { Policy = null }, CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Hold, missing.Disposition);
        Assert.Contains("lifecycle_policy_unavailable", missing.Reasons);
        var conflict = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(policy: LifecycleTestData.Policy with
            {
                EnvironmentPolicy = LifecycleTestData.Layer() with { Minimum = 900 },
                OperatorControls = LifecycleTestData.Layer() with { Maximum = 800 }
            }), CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Rejected, conflict.Disposition);
        Assert.Contains("policy_intersection_empty", conflict.Reasons);
    }

    [Theory]
    [InlineData("omitted")]
    [InlineData("identity")]
    public async Task TargetAuthorityCannotBeBroadened(string scenario)
    {
        var proposal = LifecycleTestData.Proposal();
        var policy = LifecycleTestData.Policy with
        {
            EnvironmentPolicy = LifecycleTestData.Layer() with { AllowedTargets = [LifecycleTestData.Target] },
            OperatorControls = LifecycleTestData.Layer() with
            {
                AllowedTargets = [new("cohort", "other")]
            }
        };
        if (scenario == "omitted")
        {
            proposal = proposal with { Context = proposal.Context with { ControlTarget = null } };
        }
        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(proposal, policy), CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
        Assert.Contains("target_not_authorized", result.Reasons);
    }

    [Theory]
    [InlineData("definition", "definition_not_authorized")]
    [InlineData("retired", "definition_not_authorized")]
    [InlineData("actor", "actor_scope_mismatch")]
    [InlineData("workflow", "workflow_not_permitted")]
    [InlineData("rollback", "replacement_not_authorized")]
    [InlineData("policy", "policy_scope_mismatch")]
    public async Task IncompatibleInputsFailExplicitly(string scenario, string reason)
    {
        var request = LifecycleTestData.Evaluation();
        request = scenario switch
        {
            "definition" => request with
            {
                Runtime = request.Runtime with
                {
                    Identity = request.Runtime.Identity with { Revision = "other" }
                }
            },
            "retired" => request with { Intelligence = request.Intelligence with { LifecycleStatus = "retired" } },
            "actor" => request with { Actor = request.Actor with { Environment = "prod" } },
            "workflow" => request with { Intelligence = LifecycleTestData.Intelligence(strategy: true) },
            "rollback" => request with
            {
                Replacement = LifecycleReplacementKind.Rollback,
                Policy = LifecycleTestData.Policy with
                {
                    EnvironmentPolicy = LifecycleTestData.Layer() with { AllowRollback = false }
                }
            },
            "policy" => request with { Policy = LifecycleTestData.Policy with { AppId = "other" } },
            _ => throw new InvalidOperationException("Unknown test scenario.")
        };
        var result = await _evaluator.EvaluateAsync(request, CancellationToken.None);
        Assert.Equal(LifecycleDisposition.Rejected, result.Disposition);
        Assert.Contains(reason, result.Reasons);
    }

    [Theory]
    [InlineData(0.7, 0.35, 0.5, 30d, null)]
    [InlineData(0.69, 0.35, 0.5, 30d, "insufficient_evidence_quality")]
    [InlineData(0.7, 0.36, 0.5, 30d, "model_uncertainty_exceeded")]
    [InlineData(0.7, 0.35, 0.49, 30d, "expected_outcome_below_minimum")]
    [InlineData(0.7, 0.35, 0.5, 29d, "sample_size_insufficient")]
    [InlineData(0.7, null, null, null, "model_uncertainty_exceeded")]
    public async Task EvidenceThresholdsAreNarrowingAndInclusive(
        double quality, double? uncertainty, double? outcome, double? samples, string? heldReason)
    {
        var policy = LifecycleTestData.Policy with
        {
            EnvironmentPolicy = LifecycleTestData.Layer() with
            {
                MinimumEvidenceQuality = 0.7,
                MaximumModelUncertainty = 0.4,
                MinimumExpectedOutcome = 0.5,
                MinimumSampleSize = 30
            },
            OperatorControls = LifecycleTestData.Layer() with
            {
                MinimumEvidenceQuality = 0.5,
                MaximumModelUncertainty = 0.35,
                MinimumExpectedOutcome = 0.3,
                MinimumSampleSize = 10
            }
        };
        var request = WithEvidence(new DecisionEvidenceSnapshot(quality, uncertainty, outcome, samples))
            with
        { Policy = policy };
        var result = await _evaluator.EvaluateAsync(request, CancellationToken.None);
        Assert.Equal(heldReason is null ? LifecycleDisposition.Approved : LifecycleDisposition.Hold, result.Disposition);
        if (heldReason is not null)
        {
            Assert.Contains(heldReason, result.Reasons);
        }
        Assert.Equal(0.7, result.EffectivePolicy!.MinimumEvidenceQuality);
        Assert.Equal(0.35, result.EffectivePolicy.MaximumModelUncertainty);
        Assert.Equal(0.5, result.EffectivePolicy.MinimumExpectedOutcome);
        Assert.Equal(30, result.EffectivePolicy.MinimumSampleSize);
    }

    [Theory]
    [InlineData(60, false)]
    [InlineData(60.001, true)]
    [InlineData(-0.001, true)]
    public async Task EvidenceFreshnessHonorsTheExactAgeBoundary(double ageSeconds, bool held)
    {
        var request = WithEvidence(new DecisionEvidenceSnapshot(0.9));
        request = request with
        {
            Evidence = [request.Evidence[0] with { ObservedAt = LifecycleTestData.Now.AddSeconds(-ageSeconds) }],
            Policy = LifecycleTestData.Policy with
            {
                OperatorControls = LifecycleTestData.Layer() with { MaximumEvidenceAgeSeconds = 60 }
            }
        };
        var result = await _evaluator.EvaluateAsync(request, CancellationToken.None);
        Assert.Equal(held ? LifecycleDisposition.Hold : LifecycleDisposition.Approved, result.Disposition);
        Assert.Equal(held, result.Reasons.Contains("stale_evidence"));
    }

    [Theory]
    [InlineData(850, 30, LifecycleDisposition.Approved)]
    [InlineData(900, 30, LifecycleDisposition.Limited)]
    [InlineData(850, 29.999, LifecycleDisposition.Hold)]
    [InlineData(850, -1, LifecycleDisposition.Hold)]
    public async Task ActivationDeltaAndIntervalUseDurableBaselineHistory(
        double value, double elapsedSeconds, LifecycleDisposition disposition)
    {
        var baseline = new GovernedDecisionState(
            "definition", "revision", LifecycleTestData.Definition.Contract.ContractDigest,
            JsonSerializer.SerializeToElement(800), LifecycleTestData.Target,
            ActivatedAt: LifecycleTestData.Now.AddSeconds(-elapsedSeconds));
        var policy = LifecycleTestData.Policy with
        {
            EnvironmentPolicy = LifecycleTestData.Layer() with
            {
                MaximumActivationDelta = 50,
                MinimumActivationIntervalSeconds = 30
            },
            OperatorControls = LifecycleTestData.Layer() with
            {
                MaximumActivationDelta = 100,
                MinimumActivationIntervalSeconds = 10
            }
        };
        var result = await _evaluator.EvaluateAsync(
            LifecycleTestData.Evaluation(LifecycleTestData.Proposal(value: value), policy, baseline),
            CancellationToken.None);
        Assert.Equal(disposition, result.Disposition);
        Assert.Equal(50, result.EffectivePolicy!.MaximumActivationDelta);
        Assert.Equal(30, result.EffectivePolicy.MinimumActivationIntervalSeconds);
    }

    [Theory]
    [InlineData("safe", LifecycleDisposition.Approved)]
    [InlineData("other", LifecycleDisposition.Rejected)]
    public async Task FixedStringValuesRespectTheRegisteredActionSpace(string value, LifecycleDisposition disposition)
    {
        var proposal = LifecycleTestData.Proposal() with { Value = JsonSerializer.SerializeToElement(value) };
        var request = LifecycleTestData.Evaluation(proposal);
        request = request with
        {
            Runtime = request.Runtime with { ValueType = "string", NumberActionSpace = null },
            Intelligence = request.Intelligence with
            {
                ActionSpace = new DecisionActionSpaceContract(
                    "string", JsonSerializer.SerializeToElement("safe"), AllowedValues: ["safe", "fast"])
            }
        };
        Assert.Equal(disposition, (await _evaluator.EvaluateAsync(request, CancellationToken.None)).Disposition);
    }

    private static LifecyclePolicyEvaluationRequest WithEvidence(DecisionEvidenceSnapshot evidence)
    {
        var proposal = LifecycleTestData.Proposal();
        proposal = proposal with { Context = proposal.Context with { EvidenceReferences = ["evidence"] } };
        return LifecycleTestData.Evaluation(proposal) with
        {
            Evidence =
            [
                new("evidence", LifecycleTestData.Definition, LifecycleTestData.Target,
                    LifecycleTestData.Now.AddSeconds(-30), LifecycleTestData.Now.AddHours(1), evidence)
            ]
        };
    }
}
