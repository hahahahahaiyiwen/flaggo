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
    IStrategyExecutor strategyExecutor,
    IPolicyEvaluator policyEvaluator,
    ILogger<DecisionService> logger)
{
    public async Task<ServerDecisionResult> DecideAsync(
        string decisionKey,
        DecideRequest request,
        CancellationToken cancellationToken = default)
    {
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
        VerifyInputs(definition.Inputs, request.Inputs);

        var targetPlan = await targetResolver.ResolveAsync(
            definition,
            request.RuntimeTarget,
            request.RuntimeContext,
            cancellationToken);
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
        StrategyExecutionResult? execution = null;
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
            try
            {
                evidence = await evidenceProvider.GetEvidenceAsync(
                    new DecisionEvidenceRequest(
                        definition,
                        state,
                        request.RuntimeContext,
                        request.Inputs ?? []),
                    cancellationToken);
            }
            catch (EvidenceUnavailableException error)
            {
                logger.LogWarning(
                    error,
                    "Evidence is unavailable for decision {DecisionKey} and strategy {StrategyId}",
                    decisionKey,
                    state.StrategyId);
                evidence = null;
            }

            if (evidence is null && effectivePolicy?.RequiresEvidence == true)
            {
                throw RequiredEvidenceUnavailable(effectivePolicy);
            }

            execution = await strategyExecutor.ExecuteAsync(
                new StrategyExecutionRequest(
                    state,
                    request.Inputs ?? [],
                    evidence),
                cancellationToken);
            if (execution.Candidate is not null &&
                execution.Mode is "strategy" or "experiment" &&
                execution.Confidence is null)
            {
                execution = execution with
                {
                    Candidate = null,
                    Reason = "The strategy result omitted required confidence.",
                    FailureReason = "strategy_confidence_unavailable"
                };
            }

            policyDecision = await policyEvaluator.EvaluateAsync(
                new PolicyEvaluationRequest(
                    execution.Candidate,
                    state.Value,
                    definition.NumberActionSpace,
                    effectivePolicy,
                    evidence,
                    state.LastChangedAt,
                    execution.FailureReason),
                cancellationToken);
        }

        var usesFallback = state is null || !policyDecision.Approved;
        var value = usesFallback
            ? definition.FallbackValue
            : execution!.Candidate!.Value;
        VerifyValueType(definition.ValueType, value);

        var decisionId = idGenerator.CreateDecisionId();
        var auditId = idGenerator.CreateAuditId();
        var mode = usesFallback ? "fallback" : execution!.Mode;
        var confidence = usesFallback ? null : execution!.Confidence;
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
            : execution!.Reason;
        var snapshot = new DecisionSnapshot(
            definition.AppId,
            definition.Environment,
            definition.Identity,
            value,
            definition.ValueType,
            fallback,
            request.RuntimeContext,
            request.Inputs ?? [],
            request.RuntimeTarget,
            controlTarget,
            targetProvenance,
            resolutionChain,
            policy,
            evidence,
            confidence);

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
                    request.Inputs ?? [],
                    request.RuntimeTarget,
                    controlTarget,
                    targetProvenance,
                    resolutionChain,
                    policy,
                    timeProvider.GetUtcNow(),
                    evidence,
                    confidence,
                    execution?.StrategyId,
                    reason),
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
            usesFallback ? null : execution!.StrategyId);
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

    private static void VerifyInputs(
        IReadOnlyList<RegisteredSignalInput> registeredInputs,
        IReadOnlyList<SignalInput>? inputs)
    {
        if (inputs is null)
        {
            return;
        }

        var duplicate = inputs.GroupBy(
                input => input.Signal.Key,
                StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicate is not null)
        {
            throw new DecisionContractException(
                400,
                "duplicate-signal-input",
                "The inputs array contains duplicate signal keys.");
        }

        var contracts = registeredInputs.ToDictionary(input => input.Key, StringComparer.Ordinal);
        var issues = new List<ProblemIssue>();
        for (var index = 0; index < inputs.Count; index++)
        {
            var input = inputs[index];
            if (!contracts.TryGetValue(input.Signal.Key, out var contract))
            {
                issues.Add(new ProblemIssue(
                    "signal-not-declared",
                    "error",
                    $"/inputs/{index}/signal/key",
                    $"Signal {input.Signal.Key} is not declared for inference.",
                    SignalKey: input.Signal.Key));
                continue;
            }

            var typeMatches = contract.ValueType switch
            {
                "boolean" => input.Value.ValueKind is JsonValueKind.True or JsonValueKind.False,
                "number" => input.Value.ValueKind is JsonValueKind.Number,
                "string" => input.Value.ValueKind is JsonValueKind.String,
                _ => false
            };
            if (!typeMatches)
            {
                issues.Add(new ProblemIssue(
                    "signal-type-mismatch",
                    "error",
                    $"/inputs/{index}/value",
                    $"Expected a {contract.ValueType} for {input.Signal.Key}.",
                    SignalKey: input.Signal.Key));
                continue;
            }

            if (contract.ValueType == "number")
            {
                var value = input.Value.GetDouble();
                if ((contract.Minimum is not null && value < contract.Minimum) ||
                    (contract.Maximum is not null && value > contract.Maximum))
                {
                    issues.Add(new ProblemIssue(
                        "signal-range-violation",
                        "error",
                        $"/inputs/{index}/value",
                        $"Value for {input.Signal.Key} is outside its declared range.",
                        SignalKey: input.Signal.Key));
                }
            }
        }

        if (issues.Count > 0)
        {
            throw new DecisionContractException(
                422,
                "invalid-inference-input",
                "One or more inputs do not conform to the registered decision definition.",
                issues);
        }
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

            var matches = contract.ValueType switch
            {
                "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
                "number" => value.ValueKind is JsonValueKind.Number,
                "string" => value.ValueKind is JsonValueKind.String,
                _ => false
            };
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
        mode is not ("strategy" or "experiment")
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
