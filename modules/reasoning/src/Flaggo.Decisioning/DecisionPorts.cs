using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning;

public sealed record TargetResolutionDescription(
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    bool ResolutionFallbackUsed);

public sealed record TargetResolutionPlan(
    IReadOnlyList<DecisionTargetRef?> StateTargets,
    IReadOnlyList<string> ResolutionChain,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext)
{
    public TargetResolutionDescription Describe(
        DecisionTargetRef? controlTarget,
        int selectedTargetIndex)
    {
        var fallbackUsed = selectedTargetIndex > 0;
        if (controlTarget?.Type == "cohort" &&
            TryGetString(RuntimeContext, "cohort", out var claimedCohort))
        {
            var verified = string.Equals(
                claimedCohort,
                controlTarget.Id,
                StringComparison.Ordinal);
            return new TargetResolutionDescription(
            [
                new TargetResolutionProvenance(
                    "cohort",
                    controlTarget.Id,
                    verified ? "client-verified" : "server-replaced",
                    claimedCohort)
            ],
            fallbackUsed);
        }

        if (fallbackUsed && controlTarget is not null)
        {
            return new TargetResolutionDescription(
            [
                new TargetResolutionProvenance(
                    controlTarget.Type,
                    controlTarget.Id,
                    "server-derived")
            ],
            true);
        }

        return new TargetResolutionDescription(
            controlTarget is null
                ? []
                :
                [
                    new TargetResolutionProvenance(
                        controlTarget.Type,
                        controlTarget.Id,
                        "client-claimed",
                        controlTarget.Id)
                ],
            fallbackUsed);
    }

    private static bool TryGetString(
        IReadOnlyDictionary<string, JsonElement> context,
        string key,
        out string value)
    {
        if (context.TryGetValue(key, out var element) &&
            element.ValueKind == JsonValueKind.String &&
            !string.IsNullOrWhiteSpace(element.GetString()))
        {
            value = element.GetString()!;
            return true;
        }

        value = string.Empty;
        return false;
    }
}

public interface ITargetResolver
{
    Task<TargetResolutionPlan> ResolveAsync(
        DecisionTargetRef? runtimeTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext,
        CancellationToken cancellationToken);
}

public sealed class DefaultTargetResolver(
    IReadOnlyDictionary<string, string>? authoritativeCohorts = null)
    : ITargetResolver
{
    private readonly IReadOnlyDictionary<string, string> _authoritativeCohorts =
        authoritativeCohorts ??
        new Dictionary<string, string>(StringComparer.Ordinal);

    public Task<TargetResolutionPlan> ResolveAsync(
        DecisionTargetRef? runtimeTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var targets = new List<DecisionTargetRef?>();
        if (TryGetString(runtimeContext, "cohort", out var cohort))
        {
            var resolvedCohort = _authoritativeCohorts.TryGetValue(
                cohort,
                out var authoritativeCohort)
                ? authoritativeCohort
                : cohort;
            targets.Add(new DecisionTargetRef("cohort", resolvedCohort));
        }

        if (TryGetString(runtimeContext, "userId", out var userId))
        {
            targets.Add(new DecisionTargetRef("user", userId));
        }

        if (runtimeTarget is not null)
        {
            targets.Add(runtimeTarget);
        }

        if (targets.Count == 0)
        {
            targets.Add(null);
        }
        else
        {
            targets.Add(new DecisionTargetRef("global", "global"));
            targets.Add(null);
        }

        var chain = new List<string>();
        if (runtimeTarget is not null)
        {
            chain.Add($"{runtimeTarget.Type}:{runtimeTarget.Id}");
        }

        if (TryGetString(runtimeContext, "userId", out userId))
        {
            chain.Add($"user:{userId}");
        }

        if (TryGetString(runtimeContext, "cohort", out cohort))
        {
            var resolvedCohort = _authoritativeCohorts.TryGetValue(
                cohort,
                out var authoritativeCohort)
                ? authoritativeCohort
                : cohort;
            chain.Add($"cohort:{resolvedCohort}");
        }

        chain.Add("global");
        return Task.FromResult(
            new TargetResolutionPlan(
                targets,
                chain.Distinct(StringComparer.Ordinal).ToArray(),
                runtimeContext));
    }

    private static bool TryGetString(
        IReadOnlyDictionary<string, JsonElement> context,
        string key,
        out string value)
    {
        if (context.TryGetValue(key, out var element) &&
            element.ValueKind == JsonValueKind.String &&
            !string.IsNullOrWhiteSpace(element.GetString()))
        {
            value = element.GetString()!;
            return true;
        }

        value = string.Empty;
        return false;
    }
}

public sealed record DecisionEvidenceRequest(
    RegisteredDecisionDefinition Definition,
    GovernedDecisionState State,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    IReadOnlyList<SignalInput> Inputs);

public interface IEvidenceProvider
{
    Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
        DecisionEvidenceRequest request,
        CancellationToken cancellationToken);
}

public interface IEvidenceHealth
{
    Task<bool> IsAvailableAsync(CancellationToken cancellationToken);
}

public sealed class InMemoryEvidenceProvider(
    IReadOnlyDictionary<string, DecisionEvidenceSnapshot>? evidenceByStrategy = null,
    bool available = true)
    : IEvidenceProvider, IEvidenceHealth
{
    private readonly IReadOnlyDictionary<string, DecisionEvidenceSnapshot> _evidenceByStrategy =
        evidenceByStrategy ??
        new Dictionary<string, DecisionEvidenceSnapshot>(StringComparer.Ordinal);

    public Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
        DecisionEvidenceRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(
            available &&
            request.State.StrategyId is not null &&
            _evidenceByStrategy.TryGetValue(request.State.StrategyId, out var evidence)
                ? evidence
                : null);
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}

public sealed record StrategyExecutionRequest(
    GovernedDecisionState State,
    IReadOnlyList<SignalInput> Inputs,
    DecisionEvidenceSnapshot? Evidence);

public sealed record StrategyExecutionResult(
    JsonElement? Candidate,
    string Mode,
    string? StrategyId,
    ConfidenceReport? Confidence,
    string Reason,
    string? FailureReason = null);

public interface IStrategyExecutor
{
    Task<StrategyExecutionResult> ExecuteAsync(
        StrategyExecutionRequest request,
        CancellationToken cancellationToken);
}

public sealed class DeterministicStrategyExecutor : IStrategyExecutor
{
    public Task<StrategyExecutionResult> ExecuteAsync(
        StrategyExecutionRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (request.State.NumericRule is null)
        {
            return Task.FromResult(
                new StrategyExecutionResult(
                    request.State.Value,
                    request.State.Mode,
                    request.State.StrategyId,
                    null,
                    "Returned the active governed value."));
        }

        var input = request.Inputs.FirstOrDefault(value =>
            string.Equals(
                value.Signal.Key,
                request.State.NumericRule.InputSignalKey,
                StringComparison.Ordinal));
        if (input is null ||
            input.Value.ValueKind != JsonValueKind.Number ||
            !input.Value.TryGetDouble(out var inputValue) ||
            !double.IsFinite(inputValue))
        {
            return Task.FromResult(
                new StrategyExecutionResult(
                    null,
                    "strategy",
                    request.State.StrategyId,
                    null,
                    "The numeric rule did not receive a valid input.",
                    "invalid_strategy_input"));
        }

        var candidate = inputValue >= request.State.NumericRule.Threshold
            ? request.State.NumericRule.ValueAtOrAbove
            : request.State.NumericRule.ValueBelow;
        var confidence = request.Evidence is null
            ? null
            : new ConfidenceReport(
                request.Evidence.EvidenceQuality,
                request.Evidence.ModelUncertainty,
                request.Evidence.ExpectedOutcome);
        return Task.FromResult(
            new StrategyExecutionResult(
                JsonSerializer.SerializeToElement(candidate),
                "strategy",
                request.State.StrategyId,
                confidence,
                "Applied the active numeric rule strategy."));
    }
}

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
                var offset = candidateValue - request.NumberActionSpace.Minimum;
                var quotient = offset / step;
                if (Math.Abs(quotient - Math.Round(quotient)) > 1e-9)
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
            if (request.LastChangedAt is DateTimeOffset changedAt &&
                timeProvider.GetUtcNow() < changedAt.AddSeconds(cooldown))
            {
                reasons.Add("cooldown_active");
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
