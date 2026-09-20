using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Policy;

public sealed record LifecyclePolicyEvaluationRequest(
    DecisionProposal Proposal,
    LifecycleReplacementKind Replacement,
    LifecycleActor Actor,
    RuntimeDecisionDefinition Runtime,
    IntelligenceLifecycleDefinitionSnapshot Intelligence,
    LifecyclePolicyContext? Policy,
    IReadOnlyList<ProposalEvidenceSnapshot> Evidence,
    GovernedDecisionState? Baseline,
    DateTimeOffset Now);

public interface ILifecyclePolicyEvaluator
{
    Task<LifecyclePolicyDecision> EvaluateAsync(
        LifecyclePolicyEvaluationRequest request,
        CancellationToken cancellationToken);
}

public interface ILifecyclePolicyContextProvider
{
    Task<LifecyclePolicyContext?> GetAsync(
        GovernedDefinitionIdentity definition,
        CancellationToken cancellationToken);
}

public sealed class ConfiguredLifecyclePolicyContextProvider :
    ILifecyclePolicyContextProvider
{
    private readonly IReadOnlyList<LifecyclePolicyContext> _contexts;

    public ConfiguredLifecyclePolicyContextProvider(
        IReadOnlyList<LifecyclePolicyContext> contexts)
    {
        ArgumentNullException.ThrowIfNull(contexts);
        if (contexts.Any(context => context is null ||
                string.IsNullOrWhiteSpace(context.AppId) ||
                string.IsNullOrWhiteSpace(context.Environment) ||
                string.IsNullOrWhiteSpace(context.DecisionKey)) ||
            contexts.Select(context => (context.AppId, context.Environment, context.DecisionKey))
                .Distinct().Count() != contexts.Count)
        {
            throw new InvalidDataException("Lifecycle policy scopes must be complete and unique.");
        }

        foreach (var context in contexts)
        {
            LifecyclePolicyValidation.Validate(context.EnvironmentPolicy);
            LifecyclePolicyValidation.Validate(context.OperatorControls);
        }

        _contexts = LifecycleJson.Copy(contexts.ToArray());
    }

    public Task<LifecyclePolicyContext?> GetAsync(
        GovernedDefinitionIdentity definition,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var context = _contexts.SingleOrDefault(item =>
            item.AppId == definition.AppId &&
            item.Environment == definition.Environment &&
            item.DecisionKey == definition.DecisionKey);
        return Task.FromResult(context is null ? null : LifecycleJson.Copy(context));
    }
}

public static class LifecyclePolicyValidation
{
    public static void Validate(LifecyclePolicyLayer layer)
    {
        if (layer is null || string.IsNullOrWhiteSpace(layer.Revision) ||
            layer.AllowedTargetKinds is null ||
            layer.AllowedTargetKinds.Any(string.IsNullOrWhiteSpace) ||
            layer.AllowedTargetKinds.Distinct(StringComparer.Ordinal).Count() !=
                layer.AllowedTargetKinds.Count ||
            layer.AllowedTargets?.Any(target =>
                target is null ||
                string.IsNullOrWhiteSpace(target.Type) ||
                string.IsNullOrWhiteSpace(target.Id)) == true)
        {
            throw new InvalidDataException("Lifecycle policy identity or target permissions are invalid.");
        }

        var values = new[]
        {
            layer.Minimum, layer.Maximum, layer.MinimumEvidenceQuality,
            layer.MaximumModelUncertainty, layer.MinimumExpectedOutcome,
            layer.MinimumSampleSize, layer.MaximumActivationDelta,
            layer.MinimumActivationIntervalSeconds, layer.MaximumEvidenceAgeSeconds
        };
        if (values.Any(value => value is double number && !double.IsFinite(number)) ||
            layer.Minimum > layer.Maximum ||
            layer.MinimumEvidenceQuality is < 0 or > 1 ||
            layer.MaximumModelUncertainty is < 0 or > 1 ||
            layer.MinimumExpectedOutcome is < 0 or > 1 ||
            layer.MinimumSampleSize < 0 ||
            layer.MaximumActivationDelta < 0 ||
            layer.MinimumActivationIntervalSeconds < 0 ||
            layer.MaximumEvidenceAgeSeconds < 0)
        {
            throw new InvalidDataException("Lifecycle policy constraints must be finite and coherent.");
        }
    }
}

public sealed class DefaultLifecyclePolicyEvaluator : ILifecyclePolicyEvaluator
{
    public Task<LifecyclePolicyDecision> EvaluateAsync(
        LifecyclePolicyEvaluationRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var rejected = new List<string>();
        var held = new List<string>();
        var limited = new List<string>();
        try
        {
            DecisionProposalValidation.Validate(request.Proposal);
        }
        catch (DecisionProposalValidationException error)
        {
            return Task.FromResult(new LifecyclePolicyDecision(
                LifecycleDisposition.Rejected, [error.Code], null));
        }

        var context = request.Proposal.Context;
        var definition = context.Definition;
        var runtime = request.Runtime;
        var intelligence = request.Intelligence;
        if (runtime.AppId != definition.AppId ||
            runtime.Environment != definition.Environment ||
            runtime.DecisionKey != definition.DecisionKey ||
            runtime.Identity != definition.Contract ||
            intelligence.AppId != definition.AppId ||
            intelligence.Environment != definition.Environment ||
            intelligence.DecisionKey != definition.DecisionKey ||
            intelligence.Identity != definition.Contract ||
            runtime.LifecycleStatus != "active" ||
            intelligence.LifecycleStatus != "active")
        {
            rejected.Add("definition_not_authorized");
        }

        if (request.Actor.AppId != definition.AppId ||
            request.Actor.Environment != definition.Environment ||
            string.IsNullOrWhiteSpace(request.Actor.Subject) ||
            string.IsNullOrWhiteSpace(request.Actor.Issuer))
        {
            rejected.Add("actor_scope_mismatch");
        }

        if (context.CreatedAt > request.Now)
        {
            rejected.Add("proposal_from_future");
        }
        if (context.ExpiresAt <= request.Now)
        {
            rejected.Add("expired-proposal");
        }

        var mode = intelligence.WorkflowPermissions.Mode;
        if (request.Proposal is FixedValueDecisionProposal && mode != "active-value" ||
            request.Proposal is NumericStrategyDecisionProposal && mode != "approved-strategy")
        {
            rejected.Add("workflow_not_permitted");
        }

        if (request.Policy is not { } policy)
        {
            return Task.FromResult(new LifecyclePolicyDecision(
                rejected.Count > 0 ? LifecycleDisposition.Rejected : LifecycleDisposition.Hold,
                [.. rejected, "lifecycle_policy_unavailable"],
                null));
        }

        if (policy.AppId != definition.AppId ||
            policy.Environment != definition.Environment ||
            policy.DecisionKey != definition.DecisionKey)
        {
            rejected.Add("policy_scope_mismatch");
        }

        LifecyclePolicyValidation.Validate(policy.EnvironmentPolicy);
        LifecyclePolicyValidation.Validate(policy.OperatorControls);
        var environment = policy.EnvironmentPolicy;
        var controls = policy.OperatorControls;
        var safety = intelligence.SafetyEnvelope;
        var action = intelligence.ActionSpace;
        var effective = new LifecyclePolicyLayer(
            LifecycleJson.Digest(new { Definition = intelligence, runtime.TargetHierarchy, Policy = policy }),
            runtime.TargetHierarchy.Intersect(environment.AllowedTargetKinds, StringComparer.Ordinal)
                .Intersect(controls.AllowedTargetKinds, StringComparer.Ordinal)
                .Order(StringComparer.Ordinal).ToArray(),
            environment.AutomaticApprovalAllowed && controls.AutomaticApprovalAllowed,
            environment.AllowSupersession && controls.AllowSupersession,
            environment.AllowRollback && controls.AllowRollback,
            IntersectTargets(environment.AllowedTargets, controls.AllowedTargets),
            Maximum(action.Minimum, safety.Minimum, environment.Minimum, controls.Minimum),
            Minimum(action.Maximum, safety.Maximum, environment.Maximum, controls.Maximum),
            Maximum(safety.MinimumEvidenceQuality, environment.MinimumEvidenceQuality, controls.MinimumEvidenceQuality),
            Minimum(safety.MaximumModelUncertainty, environment.MaximumModelUncertainty, controls.MaximumModelUncertainty),
            Maximum(safety.MinimumExpectedOutcome, environment.MinimumExpectedOutcome, controls.MinimumExpectedOutcome),
            Maximum(safety.MinimumSampleSize, environment.MinimumSampleSize, controls.MinimumSampleSize),
            Minimum(environment.MaximumActivationDelta, controls.MaximumActivationDelta),
            Maximum(environment.MinimumActivationIntervalSeconds, controls.MinimumActivationIntervalSeconds),
            Minimum(environment.MaximumEvidenceAgeSeconds, controls.MaximumEvidenceAgeSeconds),
            safety.Paused || environment.Paused || controls.Paused);

        if (effective.Minimum > effective.Maximum)
        {
            rejected.Add("policy_intersection_empty");
        }
        if (context.ControlTarget is not { } target ||
            !effective.AllowedTargetKinds.Contains(target.Type, StringComparer.Ordinal) ||
            effective.AllowedTargets is not null && !effective.AllowedTargets.Contains(target))
        {
            rejected.Add("target_not_authorized");
        }
        if (!Enum.IsDefined(request.Replacement) ||
            request.Replacement == LifecycleReplacementKind.Rollback && !effective.AllowRollback ||
            request.Baseline?.LifecycleStatus == GovernedDecisionStateStatus.Active &&
            request.Replacement == LifecycleReplacementKind.Supersede && !effective.AllowSupersession)
        {
            rejected.Add("replacement_not_authorized");
        }
        if (effective.Paused)
        {
            held.Add("lifecycle_paused");
        }

        var initial = request.Proposal switch
        {
            FixedValueDecisionProposal fixedValue => fixedValue.Value,
            NumericStrategyDecisionProposal strategy => strategy.InitialValue,
            _ => throw new InvalidOperationException("The validated proposal kind is unsupported.")
        };
        ValidateValue(initial, action, effective, rejected, limited);
        if (request.Proposal is NumericStrategyDecisionProposal numeric)
        {
            var inputs = (numeric.Strategy.WeightedInputs ?? [])
                .Select(input => input.SignalKey).Append(numeric.Strategy.InputSignalKey);
            if (inputs.Any(key =>
                    !intelligence.WorkflowPermissions.LiveInputs.Contains(key, StringComparer.Ordinal) ||
                    !intelligence.SignalRoles.Allowed.Contains(key, StringComparer.Ordinal) ||
                    !runtime.Inputs.Any(input => input.Key == key && input.ValueType == "number")))
            {
                rejected.Add("strategy_input_not_authorized");
            }
            foreach (var value in new[] { numeric.Strategy.ValueBelow, numeric.Strategy.ValueAtOrAbove })
            {
                ValidateValue(JsonSerializer.SerializeToElement(value), action, effective, rejected, limited);
                if (safety.MaximumDelta is double delta &&
                    Math.Abs(value - numeric.InitialValue.GetDouble()) > delta)
                {
                    rejected.Add("strategy_baseline_delta_exceeded");
                }
            }
        }

        if (effective.MaximumActivationDelta is double activationDelta &&
            request.Baseline is not null)
        {
            if (initial.ValueKind != JsonValueKind.Number ||
                request.Baseline.Value.ValueKind != JsonValueKind.Number)
            {
                rejected.Add("activation_delta_type_mismatch");
            }
            else if (Math.Abs(initial.GetDouble() - request.Baseline.Value.GetDouble()) > activationDelta)
            {
                limited.Add("activation_delta_exceeded");
            }
        }
        if (effective.MinimumActivationIntervalSeconds is double interval &&
            request.Baseline is not null)
        {
            if (request.Baseline.ActivatedAt is not DateTimeOffset activatedAt)
            {
                held.Add("activation_history_unavailable");
            }
            else if (activatedAt > request.Now || (request.Now - activatedAt).TotalSeconds < interval)
            {
                held.Add("activation_cooldown_active");
            }
        }

        ValidateEvidence(request, effective, rejected, held);
        var disposition = rejected.Count > 0 ? LifecycleDisposition.Rejected
            : held.Count > 0 ? LifecycleDisposition.Hold
            : limited.Count > 0 ? LifecycleDisposition.Limited
            : !effective.AutomaticApprovalAllowed ? LifecycleDisposition.PendingApproval
            : LifecycleDisposition.Approved;
        if (disposition == LifecycleDisposition.PendingApproval)
        {
            held.Add("human_approval_required");
        }

        return Task.FromResult(new LifecyclePolicyDecision(
            disposition,
            rejected.Concat(held).Concat(limited).Distinct(StringComparer.Ordinal).ToArray(),
            effective));
    }

    private static void ValidateValue(
        JsonElement value,
        DecisionActionSpaceContract action,
        LifecyclePolicyLayer effective,
        List<string> rejected,
        List<string> limited)
    {
        var matchesType = action.ValueType switch
        {
            "number" => value.ValueKind == JsonValueKind.Number,
            "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "string" => value.ValueKind == JsonValueKind.String,
            _ => false
        };
        if (!matchesType)
        {
            rejected.Add("candidate_type_mismatch");
            return;
        }
        if (action.ValueType == "string" && action.AllowedValues is { Count: > 0 } allowed &&
            !allowed.Contains(value.GetString(), StringComparer.Ordinal))
        {
            rejected.Add("candidate_out_of_bounds");
        }
        if (action.ValueType != "number")
        {
            return;
        }

        var number = value.GetDouble();
        if (number < action.Minimum || number > action.Maximum)
        {
            rejected.Add("candidate_out_of_bounds");
        }
        if (action.Step is double step &&
            !NumericPolicy.IsOnGrid(number, action.Minimum ?? 0, step))
        {
            rejected.Add("candidate_step_mismatch");
        }
        if (number < effective.Minimum || number > effective.Maximum)
        {
            limited.Add("effective_bounds_exceeded");
        }
    }

    private static void ValidateEvidence(
        LifecyclePolicyEvaluationRequest request,
        LifecyclePolicyLayer effective,
        List<string> rejected,
        List<string> held)
    {
        var context = request.Proposal.Context;
        var references = context.EvidenceReferences.Concat(context.ConfidenceReferences)
            .ToHashSet(StringComparer.Ordinal);
        var snapshots = request.Evidence;
        foreach (var snapshot in snapshots)
        {
            ProposalEvidenceValidation.Validate(snapshot);
        }
        if (snapshots.Select(item => item.Reference).Distinct(StringComparer.Ordinal).Count() != snapshots.Count ||
            snapshots.Any(item => !references.Contains(item.Reference)))
        {
            rejected.Add("evidence_reference_mismatch");
        }
        var required = effective.MinimumEvidenceQuality is not null ||
            effective.MaximumModelUncertainty is not null ||
            effective.MinimumExpectedOutcome is not null ||
            effective.MinimumSampleSize is not null ||
            effective.MaximumEvidenceAgeSeconds is not null;
        if (references.Count > snapshots.Count || required && snapshots.Count == 0)
        {
            held.Add("required_evidence_unavailable");
        }
        foreach (var snapshot in snapshots)
        {
            if (snapshot.Definition != context.Definition ||
                snapshot.ControlTarget != context.ControlTarget)
            {
                rejected.Add("evidence_scope_mismatch");
            }
            if (!ProposalEvidenceValidation.IsCurrent(snapshot, request.Now, effective.MaximumEvidenceAgeSeconds))
            {
                held.Add("stale_evidence");
            }

            var evidence = snapshot.Evidence;
            if (effective.MinimumEvidenceQuality is double quality && evidence.EvidenceQuality < quality)
            {
                held.Add("insufficient_evidence_quality");
            }
            if (effective.MaximumModelUncertainty is double uncertainty &&
                (evidence.ModelUncertainty is null || evidence.ModelUncertainty > uncertainty))
            {
                held.Add("model_uncertainty_exceeded");
            }
            if (effective.MinimumExpectedOutcome is double outcome &&
                (evidence.ExpectedOutcome is null || evidence.ExpectedOutcome < outcome))
            {
                held.Add("expected_outcome_below_minimum");
            }
            if (effective.MinimumSampleSize is double size &&
                (evidence.SampleSize is null || evidence.SampleSize < size))
            {
                held.Add("sample_size_insufficient");
            }
        }
    }

    private static IReadOnlyList<DecisionTargetRef>? IntersectTargets(
        IReadOnlyList<DecisionTargetRef>? first,
        IReadOnlyList<DecisionTargetRef>? second) =>
        first is null ? second?.ToArray()
            : second is null ? first.ToArray()
            : first.Intersect(second).ToArray();

    private static double? Minimum(params double?[] values) =>
        values.Where(value => value is not null).Min();

    private static double? Maximum(params double?[] values) =>
        values.Where(value => value is not null).Max();
}
