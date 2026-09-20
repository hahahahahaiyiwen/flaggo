using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Policy;

public sealed record PolicyEvaluationRequest(
    JsonElement? Candidate,
    JsonElement CurrentValue,
    NumberActionSpaceContract? NumberActionSpace,
    DecisionPolicyContract? Policy,
    DecisionEvidenceSnapshot? Evidence,
    DateTimeOffset? LastChangedAt,
    string? StrategyFailureReason);

public sealed record PolicyDecision(
    bool Approved,
    PolicyEvaluationResult Result);

public interface IPolicyEvaluator
{
    Task<PolicyDecision> EvaluateAsync(
        PolicyEvaluationRequest request,
        CancellationToken cancellationToken);
}

public sealed class DefaultPolicyEvaluator(TimeProvider timeProvider) : IPolicyEvaluator
{
    public Task<PolicyDecision> EvaluateAsync(
        PolicyEvaluationRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var reasons = new List<string>();
        var applied = new List<string>();
        if (request.StrategyFailureReason is not null)
        {
            reasons.Add(request.StrategyFailureReason);
        }

        var policy = request.Policy;
        if (policy?.Paused == true)
        {
            applied.Add("pause");
            reasons.Add("decision_paused");
        }

        if (request.Candidate is JsonElement candidate &&
            candidate.ValueKind == JsonValueKind.Number &&
            candidate.TryGetDouble(out var candidateValue) &&
            request.CurrentValue.ValueKind == JsonValueKind.Number &&
            request.CurrentValue.TryGetDouble(out var currentValue))
        {
            var minimum = policy?.Minimum ?? request.NumberActionSpace?.Minimum;
            var maximum = policy?.Maximum ?? request.NumberActionSpace?.Maximum;
            if (minimum is not null || maximum is not null)
            {
                applied.Add("number-bounds");
                if (candidateValue < minimum || candidateValue > maximum)
                {
                    reasons.Add("candidate_out_of_bounds");
                }
            }

            if (request.NumberActionSpace?.Step is double step)
            {
                applied.Add("number-step");
                if (!NumericPolicy.IsOnGrid(
                        candidateValue,
                        request.NumberActionSpace.Minimum,
                        step))
                {
                    reasons.Add("candidate_step_mismatch");
                }
            }

            if (policy?.MaximumDelta is double maximumDelta)
            {
                applied.Add("max-delta");
                if (Math.Abs(candidateValue - currentValue) > maximumDelta)
                {
                    reasons.Add("max_delta_exceeded");
                }
            }
        }
        else if (request.Candidate is not null)
        {
            reasons.Add("invalid_candidate_type");
        }

        if (policy?.CooldownSeconds is double cooldown)
        {
            applied.Add("cooldown");
            if (!double.IsFinite(cooldown) || cooldown < 0)
            {
                reasons.Add("invalid_cooldown");
            }
            else if (request.LastChangedAt is DateTimeOffset changedAt)
            {
                var now = timeProvider.GetUtcNow();
                if (changedAt > now ||
                    (now - changedAt).TotalSeconds < cooldown)
                {
                    reasons.Add("cooldown_active");
                }
            }
        }

        if (policy?.MinimumEvidenceQuality is double minimumQuality)
        {
            applied.Add("min-evidence-quality");
            if (request.Evidence is null ||
                request.Evidence.EvidenceQuality < minimumQuality)
            {
                reasons.Add("insufficient_evidence_quality");
            }
        }

        if (policy?.MaximumModelUncertainty is double maximumUncertainty)
        {
            applied.Add("max-model-uncertainty");
            if (request.Evidence?.ModelUncertainty is not double uncertainty ||
                uncertainty > maximumUncertainty)
            {
                reasons.Add("model_uncertainty_exceeded");
            }
        }

        if (policy?.MinimumExpectedOutcome is double minimumOutcome)
        {
            applied.Add("min-expected-outcome");
            if (request.Evidence?.ExpectedOutcome is not double outcome ||
                outcome < minimumOutcome)
            {
                reasons.Add("expected_outcome_below_minimum");
            }
        }

        if (policy?.MinimumSampleSize is double minimumSampleSize)
        {
            applied.Add("min-sample-size");
            if (request.Evidence?.SampleSize is not double sampleSize ||
                sampleSize < minimumSampleSize)
            {
                reasons.Add("sample_size_insufficient");
            }
        }

        var uniqueReasons = reasons.Distinct(StringComparer.Ordinal).ToArray();
        var approved = uniqueReasons.Length == 0 && request.Candidate is not null;
        return Task.FromResult(
            new PolicyDecision(
                approved,
                new PolicyEvaluationResult(
                    approved ? "approved" : "blocked",
                    uniqueReasons,
                    applied.Distinct(StringComparer.Ordinal).ToArray())));
    }
}
