using System.Collections.Frozen;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Evidence;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging;

namespace Flaggo.Decisioning;

public interface IRuntimeIdGenerator
{
    string CreateDecisionId();

    string CreateAuditId();

    string CreateConfirmToken();

    string CreateExposureId();
}

public sealed class GuidRuntimeIdGenerator : IRuntimeIdGenerator
{
    public string CreateDecisionId() => $"decision_{Guid.NewGuid():N}";

    public string CreateAuditId() => $"audit_{Guid.NewGuid():N}";

    public string CreateConfirmToken() => $"confirm_{Guid.NewGuid():N}";

    public string CreateExposureId() => $"exposure_{Guid.NewGuid():N}";
}

public sealed class DecisionService(
    IRuntimeDefinitionReader definitionRegistry,
    IStateStore stateStore,
    IExposureStore exposureStore,
    IAuditSink auditSink,
    IRuntimeIdGenerator idGenerator,
    TimeProvider timeProvider,
    ITargetResolver targetResolver,
    IEvidenceProvider evidenceProvider,
    INumericRuleExecutor numericRuleExecutor,
    DecisionInputResolver inputResolver,
    IPolicyEvaluator policyEvaluator,
    ILogger<DecisionService> logger)
{
    public async Task<ServerDecisionResult> DecideAsync(
        ApplicationScope scope,
        string decisionKey,
        DecideRequest request,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(scope.TenantId) ||
            scope.AppId != request.Client.AppId || scope.Environment != request.Client.Environment)
            throw new DecisionContractException(403, "scope-mismatch", "The verified scope does not match the decision request.");
        var lookup = await definitionRegistry.ResolveRuntimeAsync(
            request.Client.AppId,
            request.Client.Environment,
            decisionKey,
            request.ExpectedContract.DefinitionId,
            request.ExpectedContract.Revision,
            cancellationToken);

        if (!lookup.DecisionKeyExists)
        {
            throw new DecisionContractException(404, "unknown-decision-key", "Decision key is not registered.");
        }

        var definition = lookup.Definition;
        if (definition is null)
        {
            throw new DecisionContractException(
                409,
                "contract-not-registered",
                "The expected definitionId/revision is not registered.");
        }

        if (string.Equals(definition.LifecycleStatus, "retired", StringComparison.Ordinal))
        {
            throw new DecisionContractException(
                409,
                "retired-definition",
                "The requested revision is retired and cannot serve decisions.");
        }

        VerifyIdentity(definition.Identity, request.ExpectedContract);
        VerifyRuntimeContext(definition.RuntimeContext, request.RuntimeContext);
        var targetPlan = await targetResolver.ResolveAsync(
            definition,
            request.RuntimeTarget,
            request.RuntimeContext,
            cancellationToken);
        var evaluatedAt = timeProvider.GetUtcNow();
        var resolvedInputs = await inputResolver.ResolveAsync(
            scope, definition, targetPlan, request.Inputs, evaluatedAt, cancellationToken);
        var callerInputs = (request.Inputs ?? FrozenDictionary<string, JsonElement>.Empty)
            .ToFrozenDictionary(pair => pair.Key, pair => pair.Value.Clone(), StringComparer.Ordinal);
        var state = await stateStore.GetActiveAsync(
            decisionKey,
            definition.Identity.DefinitionId,
            definition.Identity.Revision,
            targetPlan.StateTargets,
            cancellationToken);

        if (state is not null)
        {
            VerifyStateTarget(definition, state, targetPlan.StateTargets);
            if (!string.Equals(
                    state.ContractDigest,
                    definition.Identity.ContractDigest,
                    StringComparison.Ordinal))
            {
                throw new DecisionContractException(
                    409,
                    "contract-conflict",
                    "Governed state does not match the registered contract.");
            }
        }

        DecisionEvidenceSnapshot? evidence = null;
        JsonElement? candidate = null;
        var executionReason = "Returned the active governed value.";
        string? executionFailure = null;
        PolicyDecision policyDecision;
        if (state is null)
        {
            policyDecision = new PolicyDecision(
                false,
                new PolicyEvaluationResult(
                    "fallback",
                    ["no-governed-state"],
                    []));
        }
        else
        {
            var effectivePolicy = EffectivePolicy(
                definition.Policy,
                state.Mode);
            if (effectivePolicy?.RequiresEvidence == true)
            {
                try
                {
                    evidence = await evidenceProvider.GetEvidenceAsync(
                        new DecisionEvidenceRequest(
                            definition, state, request.RuntimeContext, resolvedInputs.Values),
                        cancellationToken);
                }
                catch (EvidenceUnavailableException error)
                {
                    logger.LogWarning(error,
                        "Policy evidence is unavailable for decision {DecisionKey} and strategy {StrategyId}",
                        decisionKey, state.StrategyId);
                }
            }

            if (evidence is null && effectivePolicy?.RequiresEvidence == true)
            {
                throw RequiredEvidenceUnavailable(effectivePolicy);
            }

            if (state.NumericRule is null)
            {
                candidate = state.Value;
            }
            else
            {
                var execution = await numericRuleExecutor.ExecuteAsync(
                    new NumericRuleExecutionRequest(definition, state.NumericRule, resolvedInputs.Values),
                    cancellationToken);
                candidate = execution.Candidate;
                executionReason = execution.Reason;
                executionFailure = execution.FailureReason;
            }

            policyDecision = await policyEvaluator.EvaluateAsync(
                new PolicyEvaluationRequest(
                    candidate,
                    state.Value,
                    definition.NumberActionSpace,
                    effectivePolicy,
                    evidence,
                    state.LastChangedAt,
                    executionFailure),
                cancellationToken);
        }

        var usesFallback = state is null || !policyDecision.Approved;
        var value = usesFallback
            ? definition.FallbackValue
            : candidate!.Value;
        VerifyValueType(definition.ValueType, value);

        var decisionId = idGenerator.CreateDecisionId();
        var auditId = idGenerator.CreateAuditId();
        var mode = usesFallback ? "fallback" : state!.NumericRule is null ? state.Mode : "strategy";
        ConfidenceReport? confidence = null;
        var controlTarget = state?.ControlTarget;
        var selectedTargetIndex = state is null
            ? -1
            : targetPlan.StateTargets.ToList().FindIndex(
                target => TargetsEqual(target, state.ControlTarget));
        var targetResolution = targetPlan.Describe(
            controlTarget,
            selectedTargetIndex);
        var hasAttributableTarget = request.RuntimeTarget is not null || controlTarget is not null;
        var confirmToken = usesFallback || !hasAttributableTarget
            ? null
            : idGenerator.CreateConfirmToken();
        var targetProvenance = targetResolution.TargetProvenance;
        var resolutionChain = targetPlan.ResolutionChain;
        var policy = policyDecision.Result;
        var fallback = new ServerFallbackInfo(
            "server",
            targetResolution.ResolutionFallbackUsed,
            usesFallback,
            usesFallback
                ? state is null
                    ? definition.FallbackReason
                    : "fallback_required"
                : targetResolution.ResolutionFallbackUsed
                    ? "resolution_fallback_broader_target"
                    : null);
        var reason = usesFallback
            ? state is null
                ? "No governed state exists; returned the configured fallback value."
                : "No safe adaptive candidate; returned the configured fallback value."
            : executionReason;
        var snapshot = new DecisionSnapshot(
            scope.TenantId,
            definition.AppId,
            definition.Environment,
            definition.Identity,
            value,
            definition.ValueType,
            fallback,
            request.RuntimeContext,
            resolvedInputs.Values,
            request.RuntimeTarget,
            controlTarget,
            targetProvenance,
            resolutionChain,
            policy,
            evidence,
            confidence,
            callerInputs,
            resolvedInputs.Provenance,
            targetPlan.ResolvedTargets ?? []);

        if (confirmToken is not null)
        {
            await exposureStore.CreatePendingAsync(
                decisionId,
                confirmToken,
                snapshot,
                cancellationToken);
        }

        try
        {
            await auditSink.RecordDecisionAsync(
                new DecisionAuditRecord(
                    auditId,
                    decisionId,
                    decisionKey,
                    definition.AppId,
                    definition.Environment,
                    definition.Identity,
                    value,
                    definition.ValueType,
                    mode,
                    fallback,
                    request.RuntimeContext,
                    resolvedInputs.Values,
                    request.RuntimeTarget,
                    controlTarget,
                    targetProvenance,
                    resolutionChain,
                    policy,
                    evaluatedAt,
                    scope.TenantId,
                    evidence,
                    confidence,
                    state?.StrategyId,
                    reason,
                    callerInputs,
                    resolvedInputs.Provenance),
                cancellationToken);
        }
        catch
        {
            if (confirmToken is not null)
            {
                await exposureStore.RemovePendingAsync(decisionId, CancellationToken.None);
            }

            throw;
        }

        logger.LogInformation(
            "Decision {DecisionId} completed for {DecisionKey} in mode {DecisionMode} with policy {PolicyResult} and audit {AuditId}",
            decisionId,
            decisionKey,
            mode,
            policy.Result,
            auditId);

        return new ServerDecisionResult(
            decisionKey,
            new DecisionDefinitionRef(
                definition.AppId,
                definition.Environment,
                decisionKey,
                definition.Identity.DefinitionId,
                definition.Identity.Revision),
            decisionId,
            value,
            definition.ValueType,
            mode,
            confidence,
            targetProvenance,
            resolutionChain,
            fallback,
            policy,
            new ContractRuntimeStatus(
                definition.Identity.DefinitionId,
                definition.Identity.Revision,
                definition.Identity.ContractDigest,
                "verified",
                definition.Identity.BundleDigest,
                request.ExpectedContract.BuildId,
                request.ExpectedContract.DeploymentId,
                "identical"),
            new ExposureDirective(confirmToken is not null, confirmToken),
            reason,
            auditId,
            request.RuntimeTarget,
            controlTarget,
            usesFallback ? null : state!.StrategyId);
    }

    private static DecisionContractException RequiredEvidenceUnavailable(
        DecisionPolicyContract policy)
    {
        var eligible = string.Equals(
            policy.RequiredEvidenceUnavailable,
            "allow",
            StringComparison.Ordinal);
        return new DecisionContractException(
            503,
            "required-evidence-unavailable",
            eligible
                ? "Required evidence is unavailable; policy permits client fallback."
                : "Required evidence is unavailable and policy forbids governed fallback.",
            clientFallback: new ClientFallbackEligibility(
                eligible,
                eligible
                    ? "policy-permitted-required-evidence-unavailable"
                    : "policy-forbids-required-evidence-unavailable"));
    }

    private static void VerifyRuntimeContext(
        IReadOnlyList<RegisteredRuntimeContextField> registeredFields,
        IReadOnlyDictionary<string, JsonElement> runtimeContext)
    {
        var contracts = registeredFields.ToDictionary(field => field.Key, StringComparer.Ordinal);
        foreach (var required in registeredFields.Where(field => field.Required))
        {
            if (!runtimeContext.ContainsKey(required.Key))
            {
                throw new DecisionContractException(
                    422,
                    "invalid-runtime-context",
                    $"Required runtime context field '{required.Key}' is missing.");
            }
        }

        foreach (var (key, value) in runtimeContext)
        {
            if (!contracts.TryGetValue(key, out var contract))
            {
                throw new DecisionContractException(
                    422,
                    "invalid-runtime-context",
                    $"Runtime context field '{key}' is not registered.");
            }

            var matches = DecisionValues.Matches(value, contract.ValueType);
            if (!matches)
            {
                throw new DecisionContractException(
                    422,
                    "invalid-runtime-context",
                    $"Runtime context field '{key}' must be a {contract.ValueType}.");
            }

            if (!string.IsNullOrWhiteSpace(contract.TargetType) &&
                value.ValueKind == JsonValueKind.String &&
                string.IsNullOrWhiteSpace(value.GetString()))
            {
                throw new DecisionContractException(
                    422,
                    "invalid-runtime-context",
                    $"Target-bearing runtime context field '{key}' must not be empty.");
            }
        }
    }

    private static void VerifyStateTarget(
        RuntimeDecisionDefinition definition,
        GovernedDecisionState state,
        IReadOnlyList<DecisionTargetRef?> permittedTargets)
    {
        if (state.ControlTarget is not null &&
            !definition.AllowsTargetKind(state.ControlTarget.Type) ||
            !permittedTargets.Any(target =>
                TargetsEqual(target, state.ControlTarget)))
        {
            throw new DecisionContractException(
                409,
                "contract-conflict",
                "Governed state control target is outside the registered definition resolution plan.");
        }
    }

    private static DecisionPolicyContract? EffectivePolicy(
        DecisionPolicyContract? policy,
        string mode) =>
        policy is not null &&
        mode is not ("strategy" or "numeric-rule" or "experiment")
            ? policy with
            {
                MinimumEvidenceQuality = null,
                MaximumModelUncertainty = null,
                MinimumExpectedOutcome = null,
                MinimumSampleSize = null
            }
            : policy;

    private static bool TargetsEqual(DecisionTargetRef? left, DecisionTargetRef? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        string.Equals(left.Type, right.Type, StringComparison.Ordinal) &&
        string.Equals(left.Id, right.Id, StringComparison.Ordinal);

    private static void VerifyIdentity(
        RuntimeContractIdentity registered,
        RuntimeContractIdentity expected)
    {
        if (!string.Equals(registered.DefinitionId, expected.DefinitionId, StringComparison.Ordinal) ||
            !string.Equals(registered.Revision, expected.Revision, StringComparison.Ordinal))
        {
            throw new DecisionContractException(
                409,
                "contract-not-registered",
                "Expected definition identity is not registered.");
        }

        if (!string.Equals(registered.ContractDigest, expected.ContractDigest, StringComparison.Ordinal))
        {
            throw new DecisionContractException(
                409,
                "contract-conflict",
                "Expected contract digest conflicts with the registered revision.");
        }
    }

    private static void VerifyValueType(string valueType, JsonElement value)
    {
        var matches = valueType switch
        {
            "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "number" => value.ValueKind is JsonValueKind.Number,
            "string" => value.ValueKind is JsonValueKind.String,
            _ => false
        };

        if (!matches)
        {
            throw new DecisionContractException(
                409,
                "contract-conflict",
                $"Registered value does not conform to declared value type '{valueType}'.");
        }
    }
}
