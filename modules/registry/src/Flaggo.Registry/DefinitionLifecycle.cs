using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Registry;

public sealed record ValidatedDefinition(
    string ContractDigest,
    string? DefinitionId = null,
    string? Revision = null);

public sealed record AcceptedDefinition(
    string DefinitionId,
    string Revision,
    string ContractDigest);

public sealed record DefinitionBundleValidationResult(
    string Status,
    IReadOnlyList<ProblemIssue> Issues,
    string? BundleDigest = null,
    string? Compatibility = null,
    IReadOnlyDictionary<string, ValidatedDefinition>? ValidatedDefinitions = null);

public sealed record RegistrationReceipt(
    string Application,
    string Environment,
    string BundleDigest,
    IReadOnlyDictionary<string, AcceptedDefinition> AcceptedDefinitions,
    string Compatibility,
    string Status,
    IReadOnlyList<ProblemIssue> Issues,
    string? BuildId = null,
    string? ArtifactDigest = null,
    IReadOnlyList<JsonElement>? Changes = null);

public sealed record RequiresApprovalResult(
    string Status,
    string ApprovalRequestId,
    string Application,
    string Environment,
    string BundleDigest,
    string Compatibility,
    string ExpiresAt,
    string SnapshotUrl,
    IReadOnlyList<JsonElement> Changes,
    IReadOnlyList<ProblemIssue> Issues,
    string? SupersedesApprovalRequestId = null);

public sealed record DefinitionBundleApplyResult(int StatusCode, object Body);

public sealed record ApprovalActor(string Subject, string? DisplayName = null);

public sealed record ApprovalDecision(ApprovalActor Actor, string? Comment = null);

public sealed record RejectionDecision(
    ApprovalActor Actor,
    string ReasonCode,
    string? Comment = null);

public sealed record DefinitionBundleApprovalResult(
    string ApprovalRequestId,
    string Application,
    string Environment,
    string BundleDigest,
    string CreatedAt,
    string ExpiresAt,
    string SnapshotUrl,
    IReadOnlyList<JsonElement> Changes,
    string Status,
    string? SupersedesApprovalRequestId = null,
    string? DecidedAt = null,
    RegistrationReceipt? Receipt = null,
    ApprovalDecision? Approval = null,
    RejectionDecision? Rejection = null,
    string? ExpiredAt = null);

public sealed record DefinitionBundleSnapshot(
    JsonElement Bundle,
    byte[] CanonicalBytes,
    string BundleDigest);

public sealed record ApproveDefinitionBundleRequest(
    string ExpectedBundleDigest,
    string? Comment = null);

public sealed record RejectDefinitionBundleRequest(
    string ExpectedBundleDigest,
    string ReasonCode,
    string? Comment = null);

public interface IDefinitionBundleManager
{
    Task<DefinitionBundleValidationResult> ValidateAsync(
        JsonElement bundle,
        CancellationToken cancellationToken = default);

    Task<DefinitionBundleApplyResult> ApplyAsync(
        string idempotencyKey,
        JsonElement bundle,
        CancellationToken cancellationToken = default);
}

public interface IDefinitionApprovalManager
{
    Task<DefinitionBundleApprovalResult?> GetApprovalAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default);

    Task<DefinitionBundleSnapshot?> GetSnapshotAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default);

    Task<DefinitionBundleApprovalResult> ApproveAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string? comment,
        CancellationToken cancellationToken = default);

    Task<DefinitionBundleApprovalResult> RejectAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string reasonCode,
        string? comment,
        CancellationToken cancellationToken = default);
}

public interface IDefinitionIdentityGenerator
{
    string CreateDefinitionId();

    string CreateRevision();

    string CreateApprovalRequestId();
}

public sealed class GuidDefinitionIdentityGenerator : IDefinitionIdentityGenerator
{
    public string CreateDefinitionId() => $"def_{Guid.NewGuid():N}";

    public string CreateRevision() => $"rev_{Guid.NewGuid():N}";

    public string CreateApprovalRequestId() => $"apr_{Guid.NewGuid():N}";
}

public sealed class SequenceDefinitionIdentityGenerator : IDefinitionIdentityGenerator
{
    private int _definition;
    private int _revision;
    private int _approval;

    public string CreateDefinitionId() =>
        $"def_generated_{Interlocked.Increment(ref _definition)}";

    public string CreateRevision() =>
        $"rev_generated_{Interlocked.Increment(ref _revision)}";

    public string CreateApprovalRequestId() =>
        $"apr_generated_{Interlocked.Increment(ref _approval)}";
}

public sealed class DefinitionLifecycleException(
    int status,
    string code,
    string message,
    IReadOnlyList<ProblemIssue>? issues = null) : Exception(message)
{
    public int Status { get; } = status;

    public string Code { get; } = code;

    public IReadOnlyList<ProblemIssue>? Issues { get; } = issues;
}

public sealed partial class InMemoryDefinitionRegistry :
    IDefinitionBundleManager,
    IDefinitionApprovalManager
{
    private static readonly TimeSpan ApprovalLifetime = TimeSpan.FromDays(7);

    private readonly Dictionary<string, ApplyEntry> _applyEntries =
        new(StringComparer.Ordinal);
    private readonly Dictionary<string, ApprovalEntry> _approvals =
        new(StringComparer.Ordinal);

    public Task<DefinitionBundleValidationResult> ValidateAsync(
        JsonElement bundle,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            return Task.FromResult(ValidateCore(bundle));
        }
    }

    public Task<DefinitionBundleApplyResult> ApplyAsync(
        string idempotencyKey,
        JsonElement bundle,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (string.IsNullOrWhiteSpace(idempotencyKey))
        {
            throw new DefinitionLifecycleException(
                400,
                "idempotency-key-required",
                "Idempotency-Key is required.");
        }

        lock (_gate)
        {
            var bundleDigest = TryBundleDigest(bundle);
            if (_applyEntries.TryGetValue(idempotencyKey, out var existing))
            {
                if (!string.Equals(
                        existing.BundleDigest,
                        bundleDigest,
                        StringComparison.Ordinal))
                {
                    throw new DefinitionLifecycleException(
                        409,
                        "bundle-idempotency-conflict",
                        "The idempotency key was already used for another canonical bundle.");
                }

                if (existing.Outcome.Body is RequiresApprovalResult pending &&
                    _approvals.TryGetValue(pending.ApprovalRequestId, out var approval) &&
                    ExpireIfNeeded(approval).Result.Status == "expired")
                {
                    return Task.FromResult(
                        CreatePendingApply(
                            idempotencyKey,
                            bundle,
                            existing.BundleDigest,
                            pending.ApprovalRequestId));
                }

                return Task.FromResult(existing.Outcome);
            }

            var validation = ValidateCore(bundle);
            if (validation.Status == "invalid")
            {
                throw new DefinitionLifecycleException(
                    422,
                    "invalid-bundle",
                    "The definition bundle is invalid.",
                    validation.Issues);
            }

            DefinitionBundleApplyResult outcome;
            if (validation.Compatibility == "new-contract-required")
            {
                outcome = CreatePendingApply(
                    idempotencyKey,
                    bundle,
                    validation.BundleDigest!,
                    supersedesApprovalRequestId: null);
            }
            else
            {
                var changes = BuildNonSemanticChanges(bundle, validation.Compatibility!);
                var receipt = BuildReceipt(
                    bundle,
                    validation,
                    changes,
                    applyDefinitions: validation.Compatibility == "metadata-only");
                outcome = new DefinitionBundleApplyResult(200, receipt);
                _applyEntries.Add(
                    idempotencyKey,
                    new ApplyEntry(validation.BundleDigest!, outcome));
            }

            return Task.FromResult(outcome);
        }
    }

    public Task<DefinitionBundleApprovalResult?> GetApprovalAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            return Task.FromResult(
                _approvals.TryGetValue(approvalRequestId, out var entry)
                    ? ExpireIfNeeded(entry).Result
                    : null);
        }
    }

    public Task<DefinitionBundleSnapshot?> GetSnapshotAsync(
        string approvalRequestId,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            if (!_approvals.TryGetValue(approvalRequestId, out var entry))
            {
                return Task.FromResult<DefinitionBundleSnapshot?>(null);
            }

            return Task.FromResult<DefinitionBundleSnapshot?>(
                new DefinitionBundleSnapshot(
                    entry.Bundle.Clone(),
                    entry.CanonicalBytes.ToArray(),
                    entry.Result.BundleDigest));
        }
    }

    public Task<DefinitionBundleApprovalResult> ApproveAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string? comment,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            var entry = GetApprovalEntry(approvalRequestId);
            entry = ExpireIfNeeded(entry);
            VerifyApprovalDigest(entry, expectedBundleDigest);
            if (entry.Result.Status == "approved")
            {
                return Task.FromResult(entry.Result);
            }

            if (entry.Result.Status == "rejected")
            {
                throw TerminalConflict();
            }

            if (entry.Result.Status == "expired")
            {
                throw new DefinitionLifecycleException(
                    410,
                    "approval-expired",
                    "The approval request has expired.");
            }

            VerifyApprovalBaseline(entry);
            var applyEntry = FindPendingApplyEntry(approvalRequestId);
            var validation = ValidateCore(entry.Bundle);
            if (validation.Status != "valid")
            {
                throw new DefinitionLifecycleException(
                    422,
                    "invalid-bundle",
                    "The approved snapshot no longer validates.",
                    validation.Issues);
            }

            var receipt = BuildReceipt(
                entry.Bundle,
                validation with { Compatibility = "new-contract-required" },
                entry.Result.Changes,
                applyDefinitions: true);
            var decidedAt = Timestamp(_timeProvider.GetUtcNow());
            var approved = entry.Result with
            {
                Status = "approved",
                DecidedAt = decidedAt,
                Receipt = receipt,
                Approval = new ApprovalDecision(actor, comment)
            };
            entry.Result = approved;
            _applyEntries[applyEntry.Key] = new ApplyEntry(
                applyEntry.Value.BundleDigest,
                new DefinitionBundleApplyResult(200, receipt));
            return Task.FromResult(approved);
        }
    }

    public Task<DefinitionBundleApprovalResult> RejectAsync(
        string approvalRequestId,
        string expectedBundleDigest,
        ApprovalActor actor,
        string reasonCode,
        string? comment,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (string.IsNullOrWhiteSpace(reasonCode))
        {
            throw new DefinitionLifecycleException(
                422,
                "invalid-rejection",
                "reasonCode is required.");
        }

        lock (_gate)
        {
            var entry = GetApprovalEntry(approvalRequestId);
            entry = ExpireIfNeeded(entry);
            VerifyApprovalDigest(entry, expectedBundleDigest);
            if (entry.Result.Status == "rejected")
            {
                return Task.FromResult(entry.Result);
            }

            if (entry.Result.Status == "approved")
            {
                throw TerminalConflict();
            }

            if (entry.Result.Status == "expired")
            {
                throw new DefinitionLifecycleException(
                    410,
                    "approval-expired",
                    "The approval request has expired.");
            }

            var rejected = entry.Result with
            {
                Status = "rejected",
                DecidedAt = Timestamp(_timeProvider.GetUtcNow()),
                Rejection = new RejectionDecision(actor, reasonCode, comment)
            };
            entry.Result = rejected;
            return Task.FromResult(rejected);
        }
    }

    private DefinitionBundleValidationResult ValidateCore(JsonElement bundle)
    {
        var issues = new List<ProblemIssue>();
        string? bundleDigest = null;
        try
        {
            bundleDigest = CanonicalJson.BundleDigest(bundle);
        }
        catch (JsonException error)
        {
            issues.Add(Issue(
                "invalid-definition",
                "/",
                $"Bundle canonicalization failed: {error.Message}"));
            return new DefinitionBundleValidationResult("invalid", issues);
        }

        if (bundle.ValueKind != JsonValueKind.Object ||
            !TryString(bundle, "format", out var format) ||
            !string.Equals(
                format,
                "flaggo.decision-definition-bundle/v1",
                StringComparison.Ordinal) ||
            !bundle.TryGetProperty("application", out var application) ||
            !TryString(application, "id", out var appId) ||
            !TryString(application, "environment", out var environment) ||
            !bundle.TryGetProperty("source", out var source) ||
            source.ValueKind != JsonValueKind.Object ||
            !bundle.TryGetProperty("definitions", out var definitions) ||
            definitions.ValueKind != JsonValueKind.Array ||
            definitions.GetArrayLength() == 0)
        {
            issues.Add(Issue(
                "invalid-definition",
                "/",
                "Bundle format, application identity, source, and at least one definition are required."));
            return new DefinitionBundleValidationResult(
                "invalid",
                issues,
                bundleDigest);
        }

        var signalDeclarations = ReadSignals(bundle, issues);
        var validated = new Dictionary<string, ValidatedDefinition>(StringComparer.Ordinal);
        var compatibility = "identical";
        var definitionIndex = 0;
        var decisionKeys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var definition in definitions.EnumerateArray())
        {
            var path = $"/definitions/{definitionIndex}";
            if (!TryString(definition, "key", out var decisionKey))
            {
                issues.Add(Issue(
                    "invalid-definition",
                    path,
                    "Decision key is required."));
                definitionIndex++;
                continue;
            }

            if (!decisionKeys.Add(decisionKey))
            {
                issues.Add(Issue(
                    "invalid-definition",
                    path,
                    "Decision keys must be unique within a bundle.",
                    decisionKey));
            }

            var digest = TryContractDigest(definition, issues, path, decisionKey);
            ValidateDefinition(
                definition,
                definitionIndex,
                decisionKey,
                signalDeclarations,
                issues);

            var suppliedId = TryString(definition, "definitionId", out var definitionId)
                ? definitionId
                : null;
            var current = FindCurrent(appId, environment, decisionKey);
            if (current is null)
            {
                compatibility = "new-contract-required";
                if (suppliedId is not null)
                {
                    var lineageExists = _definitions.Values.Any(value =>
                        string.Equals(
                            value.Runtime.Identity.DefinitionId,
                            suppliedId,
                            StringComparison.Ordinal));
                    issues.Add(Issue(
                        lineageExists
                            ? "definition-lineage-mismatch"
                            : "unknown-definition-lineage",
                        $"{path}/definitionId",
                        lineageExists
                            ? "The definition lineage belongs to another decision scope."
                            : "The definition lineage is not registered.",
                        decisionKey));
                }

                if (digest is not null)
                {
                    validated[decisionKey] = new ValidatedDefinition(digest);
                }
            }
            else
            {
                if (suppliedId is null)
                {
                    issues.Add(Issue(
                        "definition-lineage-required",
                        $"{path}/definitionId",
                        "Existing decision keys must retain their definitionId.",
                        decisionKey));
                }
                else if (!string.Equals(
                             suppliedId,
                             current.Identity.DefinitionId,
                             StringComparison.Ordinal))
                {
                    var lineageExists = _definitions.Values.Any(value =>
                        string.Equals(
                            value.Runtime.Identity.DefinitionId,
                            suppliedId,
                            StringComparison.Ordinal));
                    issues.Add(Issue(
                        lineageExists
                            ? "definition-lineage-mismatch"
                            : "unknown-definition-lineage",
                        $"{path}/definitionId",
                        lineageExists
                            ? "The definition lineage belongs to another decision scope."
                            : "The definition lineage is not registered.",
                        decisionKey));
                }

                if (digest is not null)
                {
                    if (!string.Equals(
                            digest,
                            current.Identity.ContractDigest,
                            StringComparison.Ordinal))
                    {
                        compatibility = "new-contract-required";
                        validated[decisionKey] = new ValidatedDefinition(
                            digest,
                            current.Identity.DefinitionId);
                    }
                    else
                    {
                        if (compatibility == "identical" &&
                            !string.Equals(
                                bundleDigest,
                                current.Identity.BundleDigest,
                                StringComparison.Ordinal))
                        {
                            compatibility = "metadata-only";
                        }

                        validated[decisionKey] = new ValidatedDefinition(
                            digest,
                            current.Identity.DefinitionId,
                            current.Identity.Revision);
                    }
                }
            }

            definitionIndex++;
        }

        if (issues.Any(issue => issue.Severity == "error"))
        {
            return new DefinitionBundleValidationResult(
                "invalid",
                issues,
                bundleDigest);
        }

        return new DefinitionBundleValidationResult(
            "valid",
            issues,
            bundleDigest,
            compatibility,
            validated);
    }

    private DefinitionBundleApplyResult CreatePendingApply(
        string idempotencyKey,
        JsonElement bundle,
        string bundleDigest,
        string? supersedesApprovalRequestId)
    {
        var approvalRequestId = _identityGenerator.CreateApprovalRequestId();
        var now = _timeProvider.GetUtcNow();
        var expiresAt = now.Add(ApprovalLifetime);
        var changes = BuildSemanticChanges(bundle);
        var snapshotUrl =
            $"/v1/definition-bundle-approvals/{approvalRequestId}/bundle";
        var application = bundle.GetProperty("application");
        var pending = new RequiresApprovalResult(
            "requires-approval",
            approvalRequestId,
            application.GetProperty("id").GetString()!,
            application.GetProperty("environment").GetString()!,
            bundleDigest,
            "new-contract-required",
            Timestamp(expiresAt),
            snapshotUrl,
            changes,
            [],
            supersedesApprovalRequestId);
        var result = new DefinitionBundleApprovalResult(
            approvalRequestId,
            pending.Application,
            pending.Environment,
            bundleDigest,
            Timestamp(now),
            pending.ExpiresAt,
            snapshotUrl,
            changes,
            "pending",
            supersedesApprovalRequestId);
        var canonicalBytes = CanonicalJson.NormalizeBundleBytes(bundle);
        _approvals.Add(
            approvalRequestId,
            new ApprovalEntry(
                bundle.Clone(),
                canonicalBytes,
                result,
                expiresAt,
                CaptureApprovalBaselines(bundle)));
        var outcome = new DefinitionBundleApplyResult(202, pending);
        _applyEntries[idempotencyKey] = new ApplyEntry(bundleDigest, outcome);
        return outcome;
    }

    private RegistrationReceipt BuildReceipt(
        JsonElement bundle,
        DefinitionBundleValidationResult validation,
        IReadOnlyList<JsonElement> changes,
        bool applyDefinitions)
    {
        var application = bundle.GetProperty("application");
        var appId = application.GetProperty("id").GetString()!;
        var environment = application.GetProperty("environment").GetString()!;
        var accepted = new Dictionary<string, AcceptedDefinition>(StringComparer.Ordinal);
        var replacements = new List<RegisteredDefinitionProjections>();
        foreach (var definition in bundle.GetProperty("definitions").EnumerateArray())
        {
            var key = definition.GetProperty("key").GetString()!;
            var validated = validation.ValidatedDefinitions![key];
            var current = FindCurrent(appId, environment, key);
            var definitionId = current?.Identity.DefinitionId
                ?? validated.DefinitionId
                ?? _identityGenerator.CreateDefinitionId();
            var revision = validated.Revision ?? _identityGenerator.CreateRevision();
            accepted.Add(
                key,
                new AcceptedDefinition(
                    definitionId,
                    revision,
                    validated.ContractDigest));
            if (applyDefinitions)
            {
                replacements.Add(
                    BuildDefinitionProjections(
                        bundle,
                        definition,
                        definitionId,
                        revision,
                        validated.ContractDigest,
                        validation.BundleDigest!));
            }
        }

        foreach (var replacement in replacements)
        {
            var runtime = replacement.Runtime;
            var existingKeys = _definitions
                .Where(pair =>
                    pair.Key.AppId == runtime.AppId &&
                    pair.Key.Environment == runtime.Environment &&
                    pair.Key.Key == runtime.DecisionKey &&
                    pair.Value.Runtime.LifecycleStatus == "active")
                .Select(pair => pair.Key)
                .ToArray();
            foreach (var key in existingKeys)
            {
                var existing = _definitions[key];
                _definitions[key] = existing with
                {
                    Runtime = existing.Runtime with { LifecycleStatus = "retired" },
                    Intelligence = existing.Intelligence is null
                        ? null
                        : existing.Intelligence with
                        {
                            LifecycleStatus = "retired"
                        }
                };
            }

            _definitions[(
                runtime.AppId,
                runtime.Environment,
                runtime.DecisionKey,
                runtime.Identity.DefinitionId,
                runtime.Identity.Revision)] = replacement;
            _decisionKeys.Add((
                runtime.AppId,
                runtime.Environment,
                runtime.DecisionKey));
        }

        var buildId = bundle.TryGetProperty("build", out var build) &&
                      TryString(build, "buildId", out var parsedBuildId)
            ? parsedBuildId
            : null;
        var artifactDigest = bundle.TryGetProperty("build", out build) &&
                             TryString(build, "artifactDigest", out var parsedArtifactDigest)
            ? parsedArtifactDigest
            : null;
        return new RegistrationReceipt(
            appId,
            environment,
            validation.BundleDigest!,
            accepted,
            validation.Compatibility!,
            "approved",
            validation.Issues,
            buildId,
            artifactDigest,
            changes);
    }

    private RegisteredDefinitionProjections BuildDefinitionProjections(
        JsonElement bundle,
        JsonElement definition,
        string definitionId,
        string revision,
        string contractDigest,
        string bundleDigest)
    {
        var application = bundle.GetProperty("application");
        var signals = ReadSignals(bundle, []);
        var registeredInputs = new List<RegisteredSignalInput>();
        if (definition.TryGetProperty("inference", out var inference) &&
            inference.TryGetProperty("inputs", out var inputs))
        {
            foreach (var input in inputs.EnumerateArray())
            {
                var key = input.GetProperty("key").GetString()!;
                if (signals.TryGetValue(key, out var declaration))
                {
                    registeredInputs.Add(
                        new RegisteredSignalInput(
                            key,
                            declaration.ValueType,
                            declaration.Minimum,
                            declaration.Maximum));
                }
            }
        }

        var runtimeContext = new List<RegisteredRuntimeContextField>();
        if (definition.TryGetProperty("runtimeContextSchema", out var contextSchema))
        {
            runtimeContext.AddRange(
                contextSchema.EnumerateObject().Select(
                    property => new RegisteredRuntimeContextField(
                        property.Name,
                        property.Value.GetProperty("type").GetString()!,
                        property.Value.TryGetProperty("required", out var required) &&
                        required.ValueKind == JsonValueKind.True,
                        TryString(
                            property.Value,
                            "target",
                            out var targetType)
                            ? targetType
                            : null)));
        }

        var fallback = definition.GetProperty("fallback");
        var fallbackReason = TryString(fallback, "reason", out var reason)
            ? reason
            : "configured_fallback";
        var numberActionSpace = ReadNumberActionSpace(definition);
        var policy = ReadPolicy(definition);
        var identity = new RuntimeContractIdentity(
            definitionId,
            contractDigest,
            revision,
            bundleDigest);
        var targetHierarchy = ReadStringArray(definition, "targetHierarchy");
        var inferenceTarget = inference.ValueKind == JsonValueKind.Object &&
                              TryString(inference, "target", out var target)
            ? target
            : null;
        var fallbackOrder = inference.ValueKind == JsonValueKind.Object
            ? ReadStringArray(inference, "fallbackOrder")
            : null;
        var runtime = new RuntimeDecisionDefinition(
            application.GetProperty("id").GetString()!,
            application.GetProperty("environment").GetString()!,
            definition.GetProperty("key").GetString()!,
            identity,
            definition.GetProperty("valueType").GetString()!,
            fallback.GetProperty("value").Clone(),
            fallbackReason,
            registeredInputs,
            runtimeContext,
            NumberActionSpace: numberActionSpace,
            Policy: policy,
            TargetHierarchy: targetHierarchy,
            InferenceTarget: inferenceTarget,
            FallbackOrder: fallbackOrder);
        var intelligence = new IntelligenceLifecycleDefinitionSnapshot(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            identity,
            runtime.LifecycleStatus,
            ReadObjectives(definition),
            ReadSignalRoles(definition),
            ReadWorkflowPermissions(definition),
            ReadActionSpace(definition),
            policy ?? new DecisionPolicyContract());
        return new RegisteredDefinitionProjections(runtime, intelligence);
    }

    private static Dictionary<string, SignalDeclaration> ReadSignals(
        JsonElement bundle,
        ICollection<ProblemIssue> issues)
    {
        var declarations = new Dictionary<string, SignalDeclaration>(StringComparer.Ordinal);
        if (!bundle.TryGetProperty("signals", out var signals) ||
            signals.ValueKind != JsonValueKind.Array)
        {
            return declarations;
        }

        var index = 0;
        foreach (var signal in signals.EnumerateArray())
        {
            if (!TryString(signal, "key", out var key) ||
                !TryString(signal, "kind", out var kind))
            {
                issues.Add(Issue(
                    "invalid-signal-schema",
                    $"/signals/{index}",
                    "Signal kind and key are required."));
                index++;
                continue;
            }

            var valueType = kind == "event"
                ? "object"
                : TryString(signal, "type", out var type)
                    ? type
                    : string.Empty;
            double? minimum = null;
            double? maximum = null;
            if (signal.TryGetProperty("range", out var range) &&
                range.ValueKind == JsonValueKind.Array &&
                range.GetArrayLength() == 2)
            {
                minimum = range[0].GetDouble();
                maximum = range[1].GetDouble();
                if (minimum > maximum)
                {
                    issues.Add(Issue(
                        "invalid-signal-schema",
                        $"/signals/{index}",
                        "Signal range minimum cannot exceed maximum.",
                        signalKey: key));
                }
            }

            declarations[key] = new SignalDeclaration(valueType, minimum, maximum);
            index++;
        }

        return declarations;
    }

    private static void ValidateDefinition(
        JsonElement definition,
        int index,
        string decisionKey,
        IReadOnlyDictionary<string, SignalDeclaration> signals,
        ICollection<ProblemIssue> issues)
    {
        var path = $"/definitions/{index}";
        var valueType = TryString(
            definition,
            "valueType",
            out var parsedValueType)
            ? parsedValueType
            : null;
        var validActionSpace =
            definition.TryGetProperty("actionSpace", out var actionSpace) &&
            valueType is not null &&
            TryString(actionSpace, "type", out var actionType) &&
            string.Equals(valueType, actionType, StringComparison.Ordinal) &&
            IsValidActionSpace(actionSpace, actionType);
        var validFallback =
            validActionSpace &&
            IsValidFallback(definition, valueType!, actionSpace);
        if (!validActionSpace ||
            !validFallback)
        {
            issues.Add(Issue(
                "invalid-definition",
                path,
                "The action space is inconsistent with the declared value type.",
                decisionKey));
        }

        if (!HasValidSignalRoles(definition, signals))
        {
            issues.Add(Issue(
                "invalid-definition",
                $"{path}/signals",
                "Signal roles must contain objects with non-empty keys.",
                decisionKey));
        }

        ValidateTargeting(definition, path, decisionKey, issues);

        var inferenceSignalKeys = new Dictionary<string, HashSet<string>>(StringComparer.Ordinal);
        if (definition.TryGetProperty("inference", out var inference) &&
            inference.TryGetProperty("inputs", out var inputs))
        {
            if (inputs.ValueKind != JsonValueKind.Array)
            {
                issues.Add(Issue(
                    "invalid-definition",
                    $"{path}/inference/inputs",
                    "Inference inputs must be an array.",
                    decisionKey));
            }
            else
            {
                var inputIndex = 0;
                foreach (var input in inputs.EnumerateArray())
                {
                    if (!TryString(input, "key", out var key) ||
                        !signals.ContainsKey(key))
                    {
                        issues.Add(Issue(
                            "unknown-signal",
                            $"{path}/inference/inputs/{inputIndex}",
                            "Inference input references an unknown signal.",
                            decisionKey,
                            key));
                    }
                    else
                    {
                        var name = key.Split('.').Last();
                        if (!inferenceSignalKeys.TryGetValue(name, out var keys))
                        {
                            keys = new HashSet<string>(StringComparer.Ordinal);
                            inferenceSignalKeys.Add(name, keys);
                        }
                        keys.Add(key);
                    }

                    inputIndex++;
                }
            }
        }

        if (definition.TryGetProperty("intent", out var intent))
        {
            var invalidObjective = !TryString(
                intent,
                "type",
                out var intentType);
            if (!invalidObjective)
            {
                invalidObjective = intentType switch
                {
                    "natural-language" =>
                        !TryString(intent, "text", out _),
                    "metric-objective" =>
                        !HasValidMetricObjectives(intent, signals),
                    _ => true
                };
            }

            if (invalidObjective)
            {
                issues.Add(Issue(
                    "invalid-objective",
                    $"{path}/intent/primary",
                    "Objective direction/target is invalid.",
                    decisionKey));
            }
        }

        if (definition.TryGetProperty("onlineStrategy", out var strategy) &&
            (strategy.ValueKind != JsonValueKind.Object ||
             !TryString(strategy, "mode", out var strategyMode) ||
             strategyMode is not (
                 "active-value" or
                 "approved-strategy" or
                 "experiment" or
                 "fallback-only") ||
             strategy.TryGetProperty("liveInputs", out var liveInputs) &&
             (liveInputs.ValueKind != JsonValueKind.Array ||
              liveInputs.EnumerateArray().Any(
                  value => value.ValueKind != JsonValueKind.String ||
                           !inferenceSignalKeys.TryGetValue(value.GetString()!, out var keys) ||
                           keys.Count != 1))))
        {
            issues.Add(Issue(
                "invalid-strategy",
                $"{path}/onlineStrategy",
                "The online strategy must reference uniquely named, declared inference inputs.",
                decisionKey));
        }

        if (!definition.TryGetProperty("policy", out var policy) ||
            policy.ValueKind != JsonValueKind.Object)
        {
            issues.Add(Issue(
                "invalid-policy",
                $"{path}/policy",
                "A policy declaration is required.",
                decisionKey));
        }
        else
        {
            var constraints = default(JsonElement);
            var validInlinePolicy =
                TryString(policy, "kind", out var policyKind) &&
                policyKind == "inline" &&
                policy.TryGetProperty("constraints", out constraints) &&
                constraints.ValueKind == JsonValueKind.Array;
            if (!validInlinePolicy)
            {
                issues.Add(Issue(
                    "invalid-policy",
                    $"{path}/policy",
                    "Referenced policies are not supported by the local policy adapter.",
                    decisionKey));
            }
            else
            {
                var invalidPolicy = false;
                foreach (var constraint in constraints.EnumerateArray())
                {
                    if (IsInvalidConstraint(constraint))
                    {
                        invalidPolicy = true;
                        break;
                    }
                }

                if (!invalidPolicy &&
                    policy.TryGetProperty("clientFallback", out var clientFallback))
                {
                    invalidPolicy =
                        clientFallback.ValueKind != JsonValueKind.Object ||
                        !TryString(
                            clientFallback,
                            "requiredEvidenceUnavailable",
                            out var unavailableBehavior) ||
                        unavailableBehavior is not ("allow" or "forbid");
                }

                if (invalidPolicy)
                {
                    issues.Add(Issue(
                        "invalid-policy",
                        $"{path}/policy",
                        "Inline policy configuration is invalid.",
                        decisionKey));
                }
            }
        }
    }

    private static bool IsValidActionSpace(
        JsonElement actionSpace,
        string actionType)
    {
        if (!actionSpace.TryGetProperty("default", out var defaultValue))
        {
            return false;
        }

        return actionType switch
        {
            "number" =>
                defaultValue.ValueKind == JsonValueKind.Number &&
                defaultValue.TryGetDouble(out var numberDefault) &&
                double.IsFinite(numberDefault) &&
                TryReadNumberActionSpace(
                    actionSpace,
                    out var minimum,
                    out var maximum,
                    out var step) &&
                IsAllowedNumber(numberDefault, minimum, maximum, step),
            "boolean" =>
                defaultValue.ValueKind is
                    JsonValueKind.True or JsonValueKind.False,
            "string" =>
                defaultValue.ValueKind == JsonValueKind.String &&
                IsValidAllowedValues(actionSpace, defaultValue.GetString()!),
            _ => false
        };
    }

    private static bool IsValidAllowedValues(
        JsonElement actionSpace,
        string defaultValue)
    {
        if (!actionSpace.TryGetProperty("allowedValues", out var allowedValues))
        {
            return true;
        }

        if (allowedValues.ValueKind != JsonValueKind.Array)
        {
            return false;
        }

        var values = allowedValues.EnumerateArray().ToArray();
        if (values.Length == 0 ||
            !values.All(value =>
                   value.ValueKind == JsonValueKind.String &&
                   !string.IsNullOrWhiteSpace(value.GetString())))
        {
            return false;
        }

        var strings = values.Select(value => value.GetString()!).ToArray();
        return strings.Distinct(StringComparer.Ordinal).Count() ==
                   strings.Length &&
               strings.Contains(defaultValue, StringComparer.Ordinal);
    }

    private static bool IsValidFallback(
        JsonElement definition,
        string valueType,
        JsonElement actionSpace)
    {
        if (!definition.TryGetProperty("fallback", out var fallback) ||
            fallback.ValueKind != JsonValueKind.Object ||
            !fallback.TryGetProperty("value", out var fallbackValue))
        {
            return false;
        }

        return valueType switch
        {
            "number" =>
                fallbackValue.ValueKind == JsonValueKind.Number &&
                fallbackValue.TryGetDouble(out var number) &&
                double.IsFinite(number) &&
                TryReadNumberActionSpace(
                    actionSpace,
                    out var minimum,
                    out var maximum,
                    out var step) &&
                IsAllowedNumber(number, minimum, maximum, step),
            "boolean" =>
                fallbackValue.ValueKind is
                    JsonValueKind.True or JsonValueKind.False,
            "string" =>
                fallbackValue.ValueKind == JsonValueKind.String &&
                IsAllowedString(
                    actionSpace,
                    fallbackValue.GetString()!),
            _ => false
        };
    }

    private static bool TryReadNumberActionSpace(
        JsonElement actionSpace,
        out double minimum,
        out double maximum,
        out double? step)
    {
        minimum = 0;
        maximum = 0;
        step = null;
        if (!TryNumber(actionSpace, "min", out minimum) ||
            !TryNumber(actionSpace, "max", out maximum) ||
            minimum > maximum)
        {
            return false;
        }

        if (actionSpace.TryGetProperty("step", out _))
        {
            if (!TryNumber(actionSpace, "step", out var parsedStep) ||
                parsedStep <= 0)
            {
                return false;
            }

            step = parsedStep;
        }

        return true;
    }

    private static bool IsAllowedNumber(
        double value,
        double minimum,
        double maximum,
        double? step)
    {
        if (value < minimum || value > maximum || step is null)
        {
            return value >= minimum && value <= maximum;
        }

        var stepCount = (value - minimum) / step.Value;
        var tolerance = 1e-9 * Math.Max(1, Math.Abs(stepCount));
        return Math.Abs(stepCount - Math.Round(stepCount)) <= tolerance;
    }

    private static bool IsAllowedString(
        JsonElement actionSpace,
        string value)
    {
        if (!actionSpace.TryGetProperty("allowedValues", out var allowedValues))
        {
            return true;
        }

        return allowedValues.EnumerateArray().Any(item =>
            string.Equals(
                item.GetString(),
                value,
                StringComparison.Ordinal));
    }

    private static bool HasValidSignalRoles(
        JsonElement definition,
        IReadOnlyDictionary<string, SignalDeclaration> signalDeclarations)
    {
        if (!definition.TryGetProperty("signals", out var signals))
        {
            return true;
        }

        if (signals.ValueKind != JsonValueKind.Object)
        {
            return false;
        }

        return new[] { "allowed", "evidence", "guardrails" }
            .All(role => HasValidSignalRole(
                signals,
                role,
                signalDeclarations));
    }

    private static bool HasValidSignalRole(
        JsonElement signals,
        string role,
        IReadOnlyDictionary<string, SignalDeclaration> signalDeclarations)
    {
        if (!signals.TryGetProperty(role, out var values))
        {
            return true;
        }

        return values.ValueKind == JsonValueKind.Array &&
               values.EnumerateArray().All(value =>
                   value.ValueKind == JsonValueKind.Object &&
                   TryString(value, "key", out var key) &&
                   signalDeclarations.ContainsKey(key));
    }

    private static bool HasValidMetricObjectives(
        JsonElement intent,
        IReadOnlyDictionary<string, SignalDeclaration> signals)
    {
        if (!intent.TryGetProperty("primary", out var primary) ||
            !IsValidObjective(primary, signals))
        {
            return false;
        }

        return !intent.TryGetProperty("secondary", out var secondary) ||
               secondary.ValueKind == JsonValueKind.Array &&
               secondary.EnumerateArray().All(objective =>
                   IsValidObjective(objective, signals));
    }

    private static bool IsValidObjective(
        JsonElement objective,
        IReadOnlyDictionary<string, SignalDeclaration> signals)
    {
        if (objective.ValueKind != JsonValueKind.Object ||
            !objective.TryGetProperty("signal", out var signal) ||
            !TryString(signal, "key", out var signalKey) ||
            !signals.TryGetValue(signalKey, out var declaration) ||
            !string.Equals(
                declaration.ValueType,
                "number",
                StringComparison.Ordinal) ||
            !TryString(objective, "direction", out var direction))
        {
            return false;
        }

        return direction switch
        {
            "target" => TryNumber(objective, "target", out _),
            "minimize" or "maximize" => !objective.TryGetProperty("target", out _),
            _ => false
        };
    }

    private static void ValidateTargeting(
        JsonElement definition,
        string path,
        string decisionKey,
        ICollection<ProblemIssue> issues)
    {
        var invalid = false;
        string[]? hierarchy = null;
        if (definition.TryGetProperty("targetHierarchy", out var hierarchyElement))
        {
            if (hierarchyElement.ValueKind != JsonValueKind.Array)
            {
                invalid = true;
            }
            else
            {
                hierarchy = hierarchyElement.EnumerateArray()
                    .Where(value => value.ValueKind == JsonValueKind.String)
                    .Select(value => value.GetString()!)
                    .ToArray();
                invalid =
                    hierarchy.Length != hierarchyElement.GetArrayLength() ||
                    hierarchy.Length == 0 ||
                    hierarchy.Any(string.IsNullOrWhiteSpace) ||
                    hierarchy.Distinct(StringComparer.Ordinal).Count() !=
                    hierarchy.Length;
            }
        }

        string? inferenceTarget = null;
        if (definition.TryGetProperty("inference", out var inference))
        {
            invalid |=
                inference.ValueKind != JsonValueKind.Object ||
                !TryString(inference, "target", out inferenceTarget);
            if (inferenceTarget is not null &&
                hierarchy is not null &&
                !hierarchy.Contains(inferenceTarget, StringComparer.Ordinal))
            {
                invalid = true;
            }

            if (inference.ValueKind == JsonValueKind.Object &&
                inference.TryGetProperty("fallbackOrder", out var fallback))
            {
                if (fallback.ValueKind != JsonValueKind.Array)
                {
                    invalid = true;
                }
                else
                {
                    var fallbackTargets = fallback.EnumerateArray()
                        .Where(value => value.ValueKind == JsonValueKind.String)
                        .Select(value => value.GetString()!)
                        .ToArray();
                    invalid |=
                        fallbackTargets.Length != fallback.GetArrayLength() ||
                        fallbackTargets.Any(string.IsNullOrWhiteSpace) ||
                        fallbackTargets.Distinct(StringComparer.Ordinal).Count() !=
                        fallbackTargets.Length ||
                        hierarchy is not null &&
                        fallbackTargets.Any(target =>
                            !hierarchy.Contains(target, StringComparer.Ordinal));
                }
            }
        }

        if (definition.TryGetProperty("runtimeContextSchema", out var contextSchema))
        {
            if (contextSchema.ValueKind != JsonValueKind.Object)
            {
                invalid = true;
            }
            else
            {
                var targetFields = new HashSet<string>(StringComparer.Ordinal);
                foreach (var property in contextSchema.EnumerateObject())
                {
                    if (property.Value.ValueKind != JsonValueKind.Object ||
                        !TryString(property.Value, "type", out var valueType) ||
                        valueType is not ("boolean" or "number" or "string"))
                    {
                        invalid = true;
                        continue;
                    }

                    if (property.Value.TryGetProperty("required", out var required) &&
                        required.ValueKind is not (
                            JsonValueKind.True or JsonValueKind.False))
                    {
                        invalid = true;
                    }

                    if (!property.Value.TryGetProperty("target", out var target))
                    {
                        continue;
                    }

                    if (target.ValueKind != JsonValueKind.String ||
                        string.IsNullOrWhiteSpace(target.GetString()))
                    {
                        invalid = true;
                        continue;
                    }

                    var targetType = target.GetString()!;
                    invalid |=
                        !string.Equals(valueType, "string", StringComparison.Ordinal) ||
                        hierarchy is not null &&
                        !hierarchy.Contains(targetType, StringComparer.Ordinal) ||
                        !targetFields.Add(targetType);
                }
            }
        }

        if (invalid)
        {
            issues.Add(Issue(
                "invalid-targeting",
                path,
                "Target hierarchy, inference fallback, and target-bearing runtime context must be consistent.",
                decisionKey));
        }
    }

    private static bool IsInvalidConstraint(JsonElement constraint)
    {
        if (!TryString(constraint, "kind", out var kind))
        {
            return true;
        }

        return kind switch
        {
            "number-bounds" =>
                !TryNumber(constraint, "min", out var minimum) ||
                !TryNumber(constraint, "max", out var maximum) ||
                minimum > maximum,
            "max-delta" =>
                !TryNumber(constraint, "value", out var delta) || delta < 0,
            "cooldown" =>
                !TryNumber(constraint, "seconds", out var seconds) || seconds < 0,
            "min-evidence-quality" or
            "max-model-uncertainty" or
            "min-expected-outcome" =>
                !TryNumber(constraint, "value", out var ratio) ||
                ratio is < 0 or > 1,
            "min-sample-size" =>
                !TryNumber(constraint, "value", out var sampleSize) ||
                sampleSize < 0,
            "pause" =>
                !constraint.TryGetProperty("paused", out var paused) ||
                paused.ValueKind is not JsonValueKind.True and not JsonValueKind.False,
            _ => true
        };
    }

    private static NumberActionSpaceContract? ReadNumberActionSpace(
        JsonElement definition)
    {
        if (!definition.TryGetProperty("actionSpace", out var actionSpace) ||
            !TryString(actionSpace, "type", out var type) ||
            type != "number" ||
            !TryNumber(actionSpace, "min", out var minimum) ||
            !TryNumber(actionSpace, "max", out var maximum))
        {
            return null;
        }

        return new NumberActionSpaceContract(
            minimum,
            maximum,
            TryNumber(actionSpace, "step", out var step) ? step : null);
    }

    private static IReadOnlyList<string>? ReadStringArray(
        JsonElement value,
        string propertyName)
    {
        if (!value.TryGetProperty(propertyName, out var array) ||
            array.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        return array.EnumerateArray()
            .Select(item => item.GetString()!)
            .ToArray();
    }

    private static DecisionObjectives ReadObjectives(JsonElement definition)
    {
        if (!definition.TryGetProperty("intent", out var intent) ||
            !TryString(intent, "type", out var type))
        {
            return new DecisionObjectives(Secondary: []);
        }

        if (type == "natural-language")
        {
            return new DecisionObjectives(
                NaturalLanguage:
                    TryString(intent, "text", out var text) ? text : null,
                Secondary: []);
        }

        var primary = intent.TryGetProperty("primary", out var primaryElement)
            ? ReadObjective(primaryElement)
            : null;
        var secondary = intent.TryGetProperty("secondary", out var secondaryElement) &&
                        secondaryElement.ValueKind == JsonValueKind.Array
            ? secondaryElement.EnumerateArray()
                .Select(ReadObjective)
                .Where(objective => objective is not null)
                .Cast<DecisionObjective>()
                .ToArray()
            : [];
        return new DecisionObjectives(
            Primary: primary,
            Secondary: secondary,
            Rationale:
                TryString(intent, "rationale", out var rationale)
                    ? rationale
                    : null);
    }

    private static DecisionObjective? ReadObjective(JsonElement objective)
    {
        if (!objective.TryGetProperty("signal", out var signal) ||
            !TryString(signal, "key", out var signalKey) ||
            !TryString(objective, "direction", out var direction))
        {
            return null;
        }

        return new DecisionObjective(
            signalKey,
            direction,
            TryNumber(objective, "target", out var target) ? target : null);
    }

    private static RegisteredDecisionSignalRoles ReadSignalRoles(
        JsonElement definition)
    {
        if (!definition.TryGetProperty("signals", out var signals) ||
            signals.ValueKind != JsonValueKind.Object)
        {
            return new RegisteredDecisionSignalRoles([], [], []);
        }

        return new RegisteredDecisionSignalRoles(
            ReadEffectiveAllowedSignals(definition, signals),
            ReadSignalRole(signals, "evidence"),
            ReadSignalRole(signals, "guardrails"));
    }

    private static IReadOnlyList<string> ReadEffectiveAllowedSignals(
        JsonElement definition,
        JsonElement signals)
    {
        var allowed = new HashSet<string>(StringComparer.Ordinal);
        foreach (var role in new[] { "evidence", "guardrails" })
        {
            allowed.UnionWith(ReadSignalRole(signals, role));
        }

        if (definition.TryGetProperty("inference", out var inference) &&
            inference.TryGetProperty("inputs", out var inputs) &&
            inputs.ValueKind == JsonValueKind.Array)
        {
            foreach (var input in inputs.EnumerateArray())
            {
                if (TryString(input, "key", out var key))
                {
                    allowed.Add(key);
                }
            }
        }

        if (definition.TryGetProperty("intent", out var intent))
        {
            AddObjectiveSignal(intent, "primary", allowed);
            if (intent.TryGetProperty("secondary", out var secondary) &&
                secondary.ValueKind == JsonValueKind.Array)
            {
                foreach (var objective in secondary.EnumerateArray())
                {
                    AddObjectiveSignal(objective, allowed);
                }
            }
        }

        return allowed.Order(StringComparer.Ordinal).ToArray();
    }

    private static void AddObjectiveSignal(
        JsonElement value,
        string propertyName,
        ISet<string> signals)
    {
        if (value.TryGetProperty(propertyName, out var objective))
        {
            AddObjectiveSignal(objective, signals);
        }
    }

    private static void AddObjectiveSignal(
        JsonElement objective,
        ISet<string> signals)
    {
        if (objective.TryGetProperty("signal", out var signal) &&
            TryString(signal, "key", out var key))
        {
            signals.Add(key);
        }
    }

    private static IReadOnlyList<string> ReadSignalRole(
        JsonElement signals,
        string role)
    {
        if (!signals.TryGetProperty(role, out var values) ||
            values.ValueKind != JsonValueKind.Array)
        {
            return [];
        }

        return values.EnumerateArray()
            .Select(value => value.GetProperty("key").GetString()!)
            .Order(StringComparer.Ordinal)
            .ToArray();
    }

    private static DecisionWorkflowPermissions ReadWorkflowPermissions(
        JsonElement definition)
    {
        if (!definition.TryGetProperty("onlineStrategy", out var strategy) ||
            !TryString(strategy, "mode", out var mode))
        {
            return new DecisionWorkflowPermissions("active-value", []);
        }

        var names = ReadStringArray(strategy, "liveInputs") ?? [];
        if (names.Count == 0)
        {
            return new DecisionWorkflowPermissions(mode, []);
        }
        var keysByName = definition.GetProperty("inference").GetProperty("inputs")
            .EnumerateArray().Select(input => input.GetProperty("key").GetString()!)
            .ToLookup(key => key.Split('.').Last(), StringComparer.Ordinal);
        return new DecisionWorkflowPermissions(mode,
            names.Select(name => keysByName[name].Distinct(StringComparer.Ordinal).Single()).ToArray());
    }

    private static DecisionActionSpaceContract ReadActionSpace(
        JsonElement definition)
    {
        var actionSpace = definition.GetProperty("actionSpace");
        var valueType = definition.GetProperty("valueType").GetString()!;
        var allowedValues =
            actionSpace.TryGetProperty("allowedValues", out var allowed) &&
            allowed.ValueKind == JsonValueKind.Array
                ? allowed.EnumerateArray()
                    .Select(value => value.GetString()!)
                    .ToArray()
                : [];
        return new DecisionActionSpaceContract(
            valueType,
            actionSpace.GetProperty("default").Clone(),
            TryNumber(actionSpace, "min", out var minimum) ? minimum : null,
            TryNumber(actionSpace, "max", out var maximum) ? maximum : null,
            TryNumber(actionSpace, "step", out var step) ? step : null,
            allowedValues);
    }

    private static DecisionPolicyContract? ReadPolicy(JsonElement definition)
    {
        if (!definition.TryGetProperty("policy", out var policy) ||
            !TryString(policy, "kind", out var policyKind) ||
            policyKind != "inline" ||
            !policy.TryGetProperty("constraints", out var constraints) ||
            constraints.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        double? minimum = null;
        double? maximum = null;
        double? maximumDelta = null;
        double? cooldown = null;
        double? minimumEvidenceQuality = null;
        double? maximumModelUncertainty = null;
        double? minimumExpectedOutcome = null;
        double? minimumSampleSize = null;
        var paused = false;
        var requiredEvidenceUnavailable = "forbid";
        foreach (var constraint in constraints.EnumerateArray())
        {
            if (!TryString(constraint, "kind", out var kind))
            {
                continue;
            }

            switch (kind)
            {
                case "number-bounds":
                    minimum = TryNumber(constraint, "min", out var min) ? min : null;
                    maximum = TryNumber(constraint, "max", out var max) ? max : null;
                    break;
                case "max-delta":
                    maximumDelta = TryNumber(constraint, "value", out var delta)
                        ? delta
                        : null;
                    break;
                case "cooldown":
                    cooldown = TryNumber(constraint, "seconds", out var seconds)
                        ? seconds
                        : null;
                    break;
                case "min-evidence-quality":
                    minimumEvidenceQuality = TryNumber(constraint, "value", out var quality)
                        ? quality
                        : null;
                    break;
                case "max-model-uncertainty":
                    maximumModelUncertainty =
                        TryNumber(constraint, "value", out var uncertainty)
                            ? uncertainty
                            : null;
                    break;
                case "min-expected-outcome":
                    minimumExpectedOutcome = TryNumber(constraint, "value", out var outcome)
                        ? outcome
                        : null;
                    break;
                case "min-sample-size":
                    minimumSampleSize = TryNumber(constraint, "value", out var sampleSize)
                        ? sampleSize
                        : null;
                    break;
                case "pause":
                    paused = constraint.TryGetProperty("paused", out var pause) &&
                             pause.ValueKind == JsonValueKind.True;
                    break;
            }
        }

        if (policy.TryGetProperty("clientFallback", out var clientFallback) &&
            TryString(
                clientFallback,
                "requiredEvidenceUnavailable",
                out var configuredRequiredEvidenceUnavailable))
        {
            requiredEvidenceUnavailable = configuredRequiredEvidenceUnavailable;
        }

        return new DecisionPolicyContract(
            minimum,
            maximum,
            maximumDelta,
            cooldown,
            minimumEvidenceQuality,
            maximumModelUncertainty,
            minimumExpectedOutcome,
            minimumSampleSize,
            paused,
            requiredEvidenceUnavailable);
    }

    private IReadOnlyList<JsonElement> BuildSemanticChanges(JsonElement bundle)
    {
        var application = bundle.GetProperty("application");
        var appId = application.GetProperty("id").GetString()!;
        var environment = application.GetProperty("environment").GetString()!;
        var changes = new List<JsonElement>();
        foreach (var definition in bundle.GetProperty("definitions").EnumerateArray())
        {
            var key = definition.GetProperty("key").GetString()!;
            var digest = CanonicalJson.ContractDigest(definition);
            var current = FindCurrent(appId, environment, key);
            if (current is null)
            {
                changes.Add(
                    JsonSerializer.SerializeToElement(
                        new { kind = "created", decisionKey = key }));
                continue;
            }

            changes.Add(
                JsonSerializer.SerializeToElement(
                    new
                    {
                        kind = "semantic-change",
                        decisionKey = key,
                        previous = new AcceptedDefinition(
                            current.Identity.DefinitionId,
                            current.Identity.Revision,
                            current.Identity.ContractDigest),
                        proposed = new
                        {
                            definitionId = current.Identity.DefinitionId,
                            contractDigest = digest
                        },
                        semanticDiff = new[]
                        {
                            new
                            {
                                op = "replace",
                                path = "/",
                                before = current.Identity.ContractDigest,
                                after = digest
                            }
                        }
                    },
                    new JsonSerializerOptions(JsonSerializerDefaults.Web)));
        }

        return changes;
    }

    private static IReadOnlyList<JsonElement> BuildNonSemanticChanges(
        JsonElement bundle,
        string compatibility) =>
        compatibility == "metadata-only"
            ? bundle.GetProperty("definitions")
                .EnumerateArray()
                .Select(
                    definition => JsonSerializer.SerializeToElement(
                        new
                        {
                            kind = "metadata-updated",
                            decisionKey = definition.GetProperty("key").GetString()
                        }))
                .ToArray()
            : [];

    private RuntimeDecisionDefinition? FindCurrent(
        string appId,
        string environment,
        string decisionKey) =>
        _definitions.Values
            .Select(definition => definition.Runtime)
            .Where(definition =>
                definition.AppId == appId &&
                definition.Environment == environment &&
                definition.DecisionKey == decisionKey &&
                definition.LifecycleStatus == "active")
            .OrderByDescending(definition => definition.Identity.Revision, StringComparer.Ordinal)
            .FirstOrDefault();

    private IReadOnlyDictionary<string, ApprovalBaseline?> CaptureApprovalBaselines(
        JsonElement bundle)
    {
        var application = bundle.GetProperty("application");
        var appId = application.GetProperty("id").GetString()!;
        var environment = application.GetProperty("environment").GetString()!;
        return bundle.GetProperty("definitions")
            .EnumerateArray()
            .ToDictionary(
                definition => definition.GetProperty("key").GetString()!,
                definition =>
                {
                    var current = FindCurrent(
                        appId,
                        environment,
                        definition.GetProperty("key").GetString()!);
                    return current is null
                        ? null
                        : new ApprovalBaseline(
                            current.Identity.DefinitionId,
                            current.Identity.Revision,
                            current.Identity.ContractDigest,
                            current.Identity.BundleDigest);
                },
                StringComparer.Ordinal);
    }

    private void VerifyApprovalBaseline(ApprovalEntry entry)
    {
        var application = entry.Bundle.GetProperty("application");
        var appId = application.GetProperty("id").GetString()!;
        var environment = application.GetProperty("environment").GetString()!;
        foreach (var (decisionKey, baseline) in entry.Baselines)
        {
            var current = FindCurrent(appId, environment, decisionKey);
            var currentBaseline = current is null
                ? null
                : new ApprovalBaseline(
                    current.Identity.DefinitionId,
                    current.Identity.Revision,
                    current.Identity.ContractDigest,
                    current.Identity.BundleDigest);
            if (currentBaseline != baseline)
            {
                throw new DefinitionLifecycleException(
                    409,
                    "approval-stale-conflict",
                    "The active definition changed after this approval was requested.");
            }
        }
    }

    private ApprovalEntry GetApprovalEntry(string approvalRequestId)
    {
        if (!_approvals.TryGetValue(approvalRequestId, out var entry))
        {
            throw new DefinitionLifecycleException(
                404,
                "approval-not-found",
                "The approval request does not exist.");
        }

        return entry;
    }

    private KeyValuePair<string, ApplyEntry> FindPendingApplyEntry(
        string approvalRequestId)
    {
        foreach (var pair in _applyEntries)
        {
            if (pair.Value.Outcome.Body is RequiresApprovalResult pending &&
                string.Equals(
                    pending.ApprovalRequestId,
                    approvalRequestId,
                    StringComparison.Ordinal))
            {
                return pair;
            }
        }

        throw new InvalidDataException(
            $"Approval request '{approvalRequestId}' has no pending apply outcome.");
    }

    private ApprovalEntry ExpireIfNeeded(ApprovalEntry entry)
    {
        if (entry.Result.Status == "pending" &&
            _timeProvider.GetUtcNow() >= entry.ExpiresAt)
        {
            entry.Result = entry.Result with
            {
                Status = "expired",
                ExpiredAt = Timestamp(_timeProvider.GetUtcNow())
            };
        }

        return entry;
    }

    private static void VerifyApprovalDigest(
        ApprovalEntry entry,
        string expectedBundleDigest)
    {
        if (!string.Equals(
                entry.Result.BundleDigest,
                expectedBundleDigest,
                StringComparison.Ordinal))
        {
            throw new DefinitionLifecycleException(
                409,
                "approval-bundle-conflict",
                "The expected bundle digest does not match the approval snapshot.");
        }
    }

    private static DefinitionLifecycleException TerminalConflict() =>
        new(
            409,
            "approval-terminal-conflict",
            "The approval request already reached the opposite terminal state.");

    private static string TryBundleDigest(JsonElement bundle)
    {
        try
        {
            return CanonicalJson.BundleDigest(bundle);
        }
        catch (JsonException error)
        {
            throw new DefinitionLifecycleException(
                422,
                "invalid-bundle",
                $"Bundle canonicalization failed: {error.Message}");
        }
    }

    private static string? TryContractDigest(
        JsonElement definition,
        ICollection<ProblemIssue> issues,
        string path,
        string decisionKey)
    {
        try
        {
            return CanonicalJson.ContractDigest(definition);
        }
        catch (JsonException error)
        {
            issues.Add(Issue(
                "invalid-definition",
                path,
                $"Definition canonicalization failed: {error.Message}",
                decisionKey));
            return null;
        }
    }

    private static ProblemIssue Issue(
        string code,
        string path,
        string message,
        string? decisionKey = null,
        string? signalKey = null) =>
        new(code, "error", path, message, decisionKey, signalKey);

    private static bool TryString(
        JsonElement value,
        string propertyName,
        out string result)
    {
        if (value.ValueKind == JsonValueKind.Object &&
            value.TryGetProperty(propertyName, out var property) &&
            property.ValueKind == JsonValueKind.String &&
            !string.IsNullOrWhiteSpace(property.GetString()))
        {
            result = property.GetString()!;
            return true;
        }

        result = string.Empty;
        return false;
    }

    private static bool TryNumber(
        JsonElement value,
        string propertyName,
        out double result)
    {
        if (value.ValueKind == JsonValueKind.Object &&
            value.TryGetProperty(propertyName, out var property) &&
            property.ValueKind == JsonValueKind.Number &&
            property.TryGetDouble(out result) &&
            double.IsFinite(result))
        {
            return true;
        }

        result = default;
        return false;
    }

    private static string Timestamp(DateTimeOffset value) =>
        value.UtcDateTime.ToString(
            "yyyy-MM-dd'T'HH:mm:ss.fff'Z'",
            System.Globalization.CultureInfo.InvariantCulture);

    private sealed record SignalDeclaration(
        string ValueType,
        double? Minimum,
        double? Maximum);

    private sealed record ApplyEntry(
        string BundleDigest,
        DefinitionBundleApplyResult Outcome);

    private sealed record ApprovalBaseline(
        string DefinitionId,
        string Revision,
        string ContractDigest,
        string? BundleDigest);

    private sealed class ApprovalEntry(
        JsonElement bundle,
        byte[] canonicalBytes,
        DefinitionBundleApprovalResult result,
        DateTimeOffset expiresAt,
        IReadOnlyDictionary<string, ApprovalBaseline?> baselines)
    {
        public JsonElement Bundle { get; } = bundle;

        public byte[] CanonicalBytes { get; } = canonicalBytes;

        public DefinitionBundleApprovalResult Result { get; set; } = result;

        public DateTimeOffset ExpiresAt { get; } = expiresAt;

        public IReadOnlyDictionary<string, ApprovalBaseline?> Baselines { get; } =
            baselines;
    }
}
