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
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    TargetResolutionProvenance? RuntimeTargetProvenance = null)
{
    public TargetResolutionDescription Describe(
        DecisionTargetRef? controlTarget,
        int selectedTargetIndex)
    {
        var fallbackUsed = controlTarget?.Type == "global" || selectedTargetIndex < 0;
        if (controlTarget is not null &&
            RuntimeTargetProvenance is not null &&
            string.Equals(
                RuntimeTargetProvenance.TargetType,
                controlTarget.Type,
                StringComparison.Ordinal) &&
            string.Equals(
                RuntimeTargetProvenance.ResolvedId,
                controlTarget.Id,
                StringComparison.Ordinal))
        {
            return new TargetResolutionDescription(
                [RuntimeTargetProvenance],
                fallbackUsed);
        }

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

        if (fallbackUsed &&
            controlTarget?.Type == "global" &&
            TryGetString(RuntimeContext, "cohort", out var fallbackClaimedCohort))
        {
            return new TargetResolutionDescription(
            [
                new TargetResolutionProvenance(
                    "cohort",
                    controlTarget.Id,
                    "server-derived",
                    fallbackClaimedCohort)
            ],
            true);
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
        TargetResolutionProvenance? runtimeTargetProvenance = null;
        if (runtimeTarget is not null)
        {
            if (runtimeTarget.Type != "cohort")
            {
                targets.Add(runtimeTarget);
            }
            else if (_authoritativeCohorts.TryGetValue(
                         runtimeTarget.Id,
                         out var authoritativeRuntimeCohort))
            {
                targets.Add(
                    new DecisionTargetRef("cohort", authoritativeRuntimeCohort));
                runtimeTargetProvenance = new TargetResolutionProvenance(
                    "cohort",
                    authoritativeRuntimeCohort,
                    string.Equals(
                        runtimeTarget.Id,
                        authoritativeRuntimeCohort,
                        StringComparison.Ordinal)
                        ? "client-verified"
                        : "server-replaced",
                    runtimeTarget.Id);
            }
        }

        if (TryGetString(runtimeContext, "userId", out var userId))
        {
            targets.Add(new DecisionTargetRef("user", userId));
        }

        if (TryGetString(runtimeContext, "cohort", out var cohort) &&
            _authoritativeCohorts.TryGetValue(cohort, out var authoritativeCohort))
        {
            targets.Add(new DecisionTargetRef("cohort", authoritativeCohort));
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
            if (runtimeTarget.Type != "cohort")
            {
                chain.Add($"{runtimeTarget.Type}:{runtimeTarget.Id}");
            }
            else if (_authoritativeCohorts.TryGetValue(
                         runtimeTarget.Id,
                         out var authoritativeRuntimeCohort))
            {
                chain.Add($"cohort:{authoritativeRuntimeCohort}");
            }
        }

        if (TryGetString(runtimeContext, "userId", out userId))
        {
            chain.Add($"user:{userId}");
        }

        if (TryGetString(runtimeContext, "cohort", out cohort) &&
            _authoritativeCohorts.TryGetValue(cohort, out authoritativeCohort))
        {
            chain.Add($"cohort:{authoritativeCohort}");
        }

        chain.Add("global");
        return Task.FromResult(
            new TargetResolutionPlan(
                targets.Distinct().ToArray(),
                chain.Distinct(StringComparer.Ordinal).ToArray(),
                runtimeContext,
                runtimeTargetProvenance));
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
