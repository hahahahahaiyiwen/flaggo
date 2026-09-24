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
    TargetResolutionProvenance? RuntimeTargetProvenance = null,
    IReadOnlyDictionary<string, string>? TargetContextKeys = null,
    IReadOnlyList<bool>? StateTargetFallbacks = null,
    IReadOnlySet<string>? AuthoritativelyResolvedTargetTypes = null,
    IReadOnlySet<int>? ServerDerivedTargetIndexes = null,
    IReadOnlyList<DecisionTargetRef>? ResolvedTargets = null,
    IReadOnlyList<TargetResolutionProvenance>? ResolvedTargetProvenance = null)
{
    public TargetResolutionDescription Describe(
        DecisionTargetRef? controlTarget,
        int selectedTargetIndex)
    {
        // Frozen v1 marks terminal global/no-state resolution, not every
        // intermediate target selected from the approved resolution chain.
        var globalResolution =
            controlTarget is null ||
            string.Equals(
                controlTarget.Type,
                "global",
                StringComparison.Ordinal);
        var fallbackUsed =
            selectedTargetIndex < 0 ||
            globalResolution &&
            StateTargetFallbacks is not null &&
            selectedTargetIndex < StateTargetFallbacks.Count &&
            StateTargetFallbacks[selectedTargetIndex];
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

        if (controlTarget is not null &&
            TryGetTargetContext(
                controlTarget.Type,
                out var claimedTarget))
        {
            var verified = string.Equals(
                claimedTarget,
                controlTarget.Id,
                StringComparison.Ordinal);
            var authoritative =
                AuthoritativelyResolvedTargetTypes?.Contains(
                    controlTarget.Type) == true;
            return new TargetResolutionDescription(
            [
                new TargetResolutionProvenance(
                    controlTarget.Type,
                    controlTarget.Id,
                    authoritative
                        ? verified
                            ? "client-verified"
                            : "server-replaced"
                        : "client-claimed",
                    claimedTarget)
            ],
            fallbackUsed);
        }

        if (fallbackUsed &&
            controlTarget?.Type == "global" &&
            selectedTargetIndex > 0 &&
            StateTargets.Take(selectedTargetIndex).Any(target =>
                string.Equals(
                    target?.Type,
                    "cohort",
                    StringComparison.Ordinal)) &&
            TryGetFallbackCohortClaim(out var fallbackClaimedCohort))
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

        if (controlTarget is not null &&
            ServerDerivedTargetIndexes?.Contains(selectedTargetIndex) == true)
        {
            return new TargetResolutionDescription(
            [
                new TargetResolutionProvenance(
                    controlTarget.Type,
                    controlTarget.Id,
                    "server-derived")
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

    private bool TryGetFallbackCohortClaim(out string value)
    {
        if (TryGetTargetContext("cohort", out value))
        {
            return true;
        }

        if (RuntimeTargetProvenance is not null &&
            string.Equals(
                RuntimeTargetProvenance.TargetType,
                "cohort",
                StringComparison.Ordinal))
        {
            value = RuntimeTargetProvenance.ClaimedId ??
                RuntimeTargetProvenance.ResolvedId;
            return true;
        }

        value = string.Empty;
        return false;
    }

    private bool TryGetTargetContext(string targetType, out string value)
    {
        if (TargetContextKeys is not null &&
            TargetContextKeys.TryGetValue(targetType, out var key))
        {
            return TryGetString(RuntimeContext, key, out value);
        }

        value = string.Empty;
        return false;
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
        RuntimeDecisionDefinition definition,
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
        RuntimeDecisionDefinition definition,
        DecisionTargetRef? runtimeTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateRuntimeTarget(definition, runtimeTarget);
        var targetContextKeys = definition.RuntimeContext
            .Where(field => !string.IsNullOrWhiteSpace(field.TargetType))
            .ToDictionary(
                field => field.TargetType!,
                field => field.Key,
                StringComparer.Ordinal);
        var contextTargets = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (targetType, key) in targetContextKeys)
        {
            if (TryGetString(runtimeContext, key, out var targetId))
            {
                contextTargets.Add(targetType, targetId);
            }
        }

        if (runtimeTarget is not null &&
            contextTargets.TryGetValue(runtimeTarget.Type, out var contextTargetId) &&
            !string.Equals(runtimeTarget.Id, contextTargetId, StringComparison.Ordinal))
        {
            throw new DecisionContractException(
                422,
                "invalid-runtime-context",
                $"Runtime context target '{runtimeTarget.Type}' does not match the runtime target.");
        }

        var targets = new List<DecisionTargetRef?>();
        var targetFallbacks = new List<bool>();
        var authoritativelyResolvedTargetTypes =
            new HashSet<string>(StringComparer.Ordinal);
        var serverDerivedTargetIndexes = new HashSet<int>();
        var chain = new List<string>();
        TargetResolutionProvenance? runtimeTargetProvenance = null;
        var appendTargetlessGlobal = false;
        var targetlessGlobalFallback = false;
        var primaryTargetType = runtimeTarget?.Type ?? definition.InferenceTarget;
        foreach (var targetType in new[] { primaryTargetType }
                     .Concat(definition.FallbackOrder)
                     .Distinct(StringComparer.Ordinal))
        {
            var isFallbackTarget = !string.Equals(
                targetType,
                primaryTargetType,
                StringComparison.Ordinal);
            if (!definition.AllowsTargetKind(targetType))
            {
                throw new DecisionContractException(
                    409,
                    "contract-conflict",
                    $"Registered fallback target '{targetType}' is outside the target hierarchy.");
            }

            DecisionTargetRef? candidate = null;
            string? claimedId = null;
            var fromRuntimeTarget =
                runtimeTarget is not null &&
                string.Equals(
                    runtimeTarget.Type,
                    targetType,
                    StringComparison.Ordinal);
            if (fromRuntimeTarget)
            {
                claimedId = runtimeTarget!.Id;
            }
            else if (contextTargets.TryGetValue(
                         targetType,
                         out var resolvedContextTargetId))
            {
                claimedId = resolvedContextTargetId;
            }
            else if (string.Equals(targetType, "global", StringComparison.Ordinal))
            {
                candidate = new DecisionTargetRef("global", "global");
            }

            if (claimedId is not null)
            {
                if (string.Equals(targetType, "cohort", StringComparison.Ordinal))
                {
                    if (_authoritativeCohorts.TryGetValue(
                            claimedId,
                            out var authoritativeCohort))
                    {
                        authoritativelyResolvedTargetTypes.Add("cohort");
                        candidate = new DecisionTargetRef(
                            "cohort",
                            authoritativeCohort);
                        if (fromRuntimeTarget)
                        {
                            runtimeTargetProvenance =
                                new TargetResolutionProvenance(
                                    "cohort",
                                    authoritativeCohort,
                                    string.Equals(
                                        claimedId,
                                        authoritativeCohort,
                                        StringComparison.Ordinal)
                                        ? "client-verified"
                                        : "server-replaced",
                                    claimedId);
                        }
                    }
                }
                else
                {
                    candidate = new DecisionTargetRef(targetType, claimedId);
                }
            }

            if (candidate is null || targets.Contains(candidate))
            {
                continue;
            }

            var serverDerivedCandidate =
                claimedId is null &&
                string.Equals(
                    targetType,
                    "global",
                    StringComparison.Ordinal);
            if (serverDerivedCandidate)
            {
                serverDerivedTargetIndexes.Add(targets.Count);
            }

            targets.Add(candidate);
            targetFallbacks.Add(isFallbackTarget);
            chain.Add(
                string.Equals(candidate.Type, "global", StringComparison.Ordinal)
                    ? "global"
                    : $"{candidate.Type}:{candidate.Id}");
            if (string.Equals(candidate.Type, "global", StringComparison.Ordinal))
            {
                appendTargetlessGlobal = true;
                targetlessGlobalFallback = isFallbackTarget;
            }
        }

        if (appendTargetlessGlobal)
        {
            serverDerivedTargetIndexes.Add(targets.Count);
            targets.Add(null);
            targetFallbacks.Add(targetlessGlobalFallback);
        }

        var resolvedTargets = new List<DecisionTargetRef>();
        var resolvedTargetProvenance = new List<TargetResolutionProvenance>();
        foreach (var type in definition.TargetHierarchy)
        {
            if (type == "global")
            {
                resolvedTargets.Add(new DecisionTargetRef("global", "global"));
                resolvedTargetProvenance.Add(new("global", "global", "server-derived"));
                continue;
            }
            var claimedId = runtimeTarget?.Type == type ? runtimeTarget.Id :
                contextTargets.GetValueOrDefault(type);
            if (claimedId is null) continue;
            if (type == "cohort")
            {
                if (_authoritativeCohorts.TryGetValue(claimedId, out var authoritative))
                {
                    resolvedTargets.Add(new DecisionTargetRef(type, authoritative));
                    resolvedTargetProvenance.Add(new(type, authoritative,
                        claimedId == authoritative ? "client-verified" : "server-replaced", claimedId));
                }
            }
            else
            {
                resolvedTargets.Add(new DecisionTargetRef(type, claimedId));
                resolvedTargetProvenance.Add(new(type, claimedId, "client-claimed", claimedId));
            }
        }

        return Task.FromResult(
            new TargetResolutionPlan(
                targets,
                chain,
                runtimeContext,
                runtimeTargetProvenance,
                targetContextKeys,
                targetFallbacks,
                authoritativelyResolvedTargetTypes,
                serverDerivedTargetIndexes,
                resolvedTargets,
                resolvedTargetProvenance));
    }

    private static void ValidateRuntimeTarget(
        RuntimeDecisionDefinition definition,
        DecisionTargetRef? runtimeTarget)
    {
        if (runtimeTarget is null)
        {
            return;
        }

        if (!definition.AllowsTargetKind(runtimeTarget.Type))
        {
            throw new DecisionContractException(
                422,
                "invalid-runtime-target",
                $"Runtime target kind '{runtimeTarget.Type}' is outside the registered target hierarchy.");
        }
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

public sealed record NumericRuleExecutionRequest(
    RuntimeDecisionDefinition Definition,
    NumericRuleStrategy Rule,
    IReadOnlyDictionary<string, JsonElement> Inputs);

public sealed record NumericRuleExecutionResult(
    JsonElement? Candidate,
    string Reason,
    string? FailureReason = null);

public interface INumericRuleExecutor
{
    Task<NumericRuleExecutionResult> ExecuteAsync(
        NumericRuleExecutionRequest request,
        CancellationToken cancellationToken);
}

public sealed class NumericRuleExecutor : INumericRuleExecutor
{
    public Task<NumericRuleExecutionResult> ExecuteAsync(
        NumericRuleExecutionRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (request.Definition.ValueType != "number") return Task.FromResult(Invalid());
        if (request.Rule.WeightedInputs is { Count: > 0 } weightedInputs)
        {
            var score = 0d;
            var totalWeight = 0d;
            foreach (var ruleInput in weightedInputs)
            {
                if (!request.Inputs.TryGetValue(ruleInput.InputKey, out var input) ||
                    !DecisionValues.Matches(input, "number") ||
                    !double.IsFinite(ruleInput.Minimum) || !double.IsFinite(ruleInput.Maximum) ||
                    ruleInput.Minimum >= ruleInput.Maximum || !double.IsFinite(ruleInput.Weight) ||
                    ruleInput.Weight < 0)
                {
                    return Task.FromResult(Invalid());
                }

                var normalized = Math.Clamp(
                    (input.GetDouble() - ruleInput.Minimum) /
                    (ruleInput.Maximum - ruleInput.Minimum),
                    0,
                    1);
                score += normalized * ruleInput.Weight;
                totalWeight += ruleInput.Weight;
            }

            if (!double.IsFinite(totalWeight) || totalWeight <= 0 || !double.IsFinite(score))
            {
                return Task.FromResult(Invalid());
            }

            score /= totalWeight;
            return Task.FromResult(
                Result(
                    score >= request.Rule.Threshold
                        ? request.Rule.ValueAtOrAbove
                        : request.Rule.ValueBelow,
                    "Applied the active weighted numeric rule strategy."));
        }

        if (!request.Inputs.TryGetValue(request.Rule.InputKey, out var scalar) ||
            !DecisionValues.Matches(scalar, "number"))
        {
            return Task.FromResult(Invalid());
        }

        var candidate = scalar.GetDouble() >= request.Rule.Threshold
            ? request.Rule.ValueAtOrAbove
            : request.Rule.ValueBelow;
        return Task.FromResult(
            Result(
                candidate,
                "Applied the active numeric rule strategy."));
    }

    private static NumericRuleExecutionResult Result(
        double candidate,
        string reason)
    {
        if (!double.IsFinite(candidate))
        {
            return Invalid();
        }
        return new NumericRuleExecutionResult(
            JsonSerializer.SerializeToElement(candidate),
            reason);
    }

    private static NumericRuleExecutionResult Invalid() =>
        new(
            null,
            "The numeric rule did not receive every required valid input.",
            "invalid_strategy_input");
}
