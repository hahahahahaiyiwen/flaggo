using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

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
    IDefinitionRegistry definitionRegistry,
    IStateStore stateStore,
    IExposureStore exposureStore,
    IAuditSink auditSink,
    IRuntimeIdGenerator idGenerator,
    TimeProvider timeProvider)
{
    public async Task<ServerDecisionResult> DecideAsync(
        string decisionKey,
        DecideRequest request,
        CancellationToken cancellationToken = default)
    {
        var lookup = await definitionRegistry.ResolveAsync(
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

        var resolutionTargets = ResolveStateTargets(request.RuntimeTarget, request.RuntimeContext);
        var state = await stateStore.GetActiveAsync(
            decisionKey,
            definition.Identity.DefinitionId,
            definition.Identity.Revision,
            resolutionTargets,
            cancellationToken);

        if (state is not null &&
            !string.Equals(state.ContractDigest, definition.Identity.ContractDigest, StringComparison.Ordinal))
        {
            throw new DecisionContractException(
                409,
                "contract-conflict",
                "Governed state does not match the registered contract.");
        }

        var usesFallback = state is null;
        var value = usesFallback ? definition.FallbackValue : state!.Value;
        VerifyValueType(definition.ValueType, value);

        var decisionId = idGenerator.CreateDecisionId();
        var auditId = idGenerator.CreateAuditId();
        var mode = usesFallback ? "fallback" : state!.Mode;
        var controlTarget = state?.ControlTarget;
        var selectedTargetIndex = state is null
            ? -1
            : resolutionTargets.ToList().FindIndex(
                target => TargetsEqual(target, state.ControlTarget));
        var resolutionFallbackUsed = selectedTargetIndex > 0;
        var hasAttributableTarget = request.RuntimeTarget is not null || controlTarget is not null;
        var confirmToken = usesFallback || !hasAttributableTarget
            ? null
            : idGenerator.CreateConfirmToken();
        var targetProvenance = ResolveTargetProvenance(
            controlTarget,
            request.RuntimeContext,
            resolutionFallbackUsed);
        var resolutionChain = ResolveChain(request.RuntimeTarget, request.RuntimeContext);
        var policy = new PolicyEvaluationResult(
            usesFallback ? "fallback" : "approved",
            usesFallback ? ["no-governed-state"] : [],
            []);
        var fallback = new ServerFallbackInfo(
            "server",
            resolutionFallbackUsed,
            usesFallback,
            usesFallback
                ? definition.FallbackReason
                : resolutionFallbackUsed
                    ? "resolution_fallback_broader_target"
                    : null);
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
            policy);

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
                    timeProvider.GetUtcNow()),
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
            null,
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
            usesFallback
                ? "No governed state exists; returned the configured fallback value."
                : "Returned the active governed value.",
            auditId,
            request.RuntimeTarget,
            controlTarget,
            state?.StrategyId);
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
        }
    }

    private static IReadOnlyList<DecisionTargetRef?> ResolveStateTargets(
        DecisionTargetRef? runtimeTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext)
    {
        var targets = new List<DecisionTargetRef?>();
        if (TryGetString(runtimeContext, "cohort", out var cohort))
        {
            targets.Add(new DecisionTargetRef("cohort", cohort));
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

        return targets;
    }

    private static IReadOnlyList<TargetResolutionProvenance> ResolveTargetProvenance(
        DecisionTargetRef? controlTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext,
        bool resolutionFallbackUsed)
    {
        if (resolutionFallbackUsed &&
            TryGetString(runtimeContext, "cohort", out var claimedCohort))
        {
            return
            [
                new TargetResolutionProvenance(
                    "cohort",
                    controlTarget?.Id ?? "global",
                    "server-derived",
                    claimedCohort)
            ];
        }

        return
        controlTarget is null
            ? []
            : [new TargetResolutionProvenance(
                controlTarget.Type,
                controlTarget.Id,
                "client-claimed",
                controlTarget.Id)];
    }

    private static bool TargetsEqual(DecisionTargetRef? left, DecisionTargetRef? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        string.Equals(left.Type, right.Type, StringComparison.Ordinal) &&
        string.Equals(left.Id, right.Id, StringComparison.Ordinal);

    private static IReadOnlyList<string> ResolveChain(
        DecisionTargetRef? runtimeTarget,
        IReadOnlyDictionary<string, JsonElement> runtimeContext)
    {
        var chain = new List<string>();
        if (runtimeTarget is not null)
        {
            chain.Add($"{runtimeTarget.Type}:{runtimeTarget.Id}");
        }

        if (TryGetString(runtimeContext, "userId", out var userId))
        {
            chain.Add($"user:{userId}");
        }

        if (TryGetString(runtimeContext, "cohort", out var cohort))
        {
            chain.Add($"cohort:{cohort}");
        }

        chain.Add("global");
        return chain.Distinct(StringComparer.Ordinal).ToArray();
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
