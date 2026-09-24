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
    public static IReadOnlyList<RegisteredDefinitionProjections> ProjectApprovedManifest(
        JsonElement bundle, RegistrationReceipt receipt)
    {
        var issues = DecisionManifest.Validate(bundle);
        if (issues.Count > 0 || receipt.Status != "approved" ||
            receipt.BundleDigest != CanonicalJson.BundleDigest(bundle) ||
            receipt.Application != bundle.GetProperty("application").GetProperty("id").GetString() ||
            receipt.Environment != bundle.GetProperty("application").GetProperty("environment").GetString())
            throw new InvalidDataException("The approved receipt does not match the manifest.");
        var decisions = bundle.GetProperty("decisions").EnumerateObject().ToArray();
        if (decisions.Length != receipt.AcceptedDefinitions.Count ||
            decisions.Any(decision => !receipt.AcceptedDefinitions.TryGetValue(decision.Name, out var accepted) ||
                string.IsNullOrWhiteSpace(accepted.DefinitionId) || string.IsNullOrWhiteSpace(accepted.Revision) ||
                accepted.ContractDigest != CanonicalJson.ContractDigest(decision.Name, decision.Value)))
            throw new InvalidDataException("The approved receipt does not bind every manifest decision exactly.");
        return decisions.Select(decision =>
        {
            var accepted = receipt.AcceptedDefinitions[decision.Name];
            return BuildDefinitionProjections(bundle, decision.Name, decision.Value,
                accepted.DefinitionId, accepted.Revision, accepted.ContractDigest, receipt.BundleDigest);
        }).ToArray();
    }

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
            var validation = ValidateCore(bundle);
            if (validation.Status == "invalid")
            {
                throw new DefinitionLifecycleException(
                    422,
                    "invalid-bundle",
                    "The decision manifest is invalid.",
                    validation.Issues);
            }
            var bundleDigest = validation.BundleDigest!;
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
        var issues = DecisionManifest.Validate(bundle);
        if (issues.Count > 0)
        {
            return new DefinitionBundleValidationResult("invalid", issues);
        }
        var bundleDigest = CanonicalJson.BundleDigest(bundle);
        var application = bundle.GetProperty("application");
        var appId = application.GetProperty("id").GetString()!;
        var environment = application.GetProperty("environment").GetString()!;
        var validated = new Dictionary<string, ValidatedDefinition>(StringComparer.Ordinal);
        var compatibility = "identical";
        foreach (var definition in bundle.GetProperty("decisions").EnumerateObject())
        {
            var decisionKey = definition.Name;
            var digest = CanonicalJson.ContractDigest(decisionKey, definition.Value);
            var current = FindCurrent(appId, environment, decisionKey);
            if (current is null)
            {
                compatibility = "new-contract-required";
                validated[decisionKey] = new ValidatedDefinition(digest);
            }
            else if (!string.Equals(digest, current.Identity.ContractDigest, StringComparison.Ordinal))
            {
                compatibility = "new-contract-required";
                validated[decisionKey] = new ValidatedDefinition(digest, current.Identity.DefinitionId);
            }
            else
            {
                if (compatibility == "identical" && bundleDigest != current.Identity.BundleDigest)
                {
                    compatibility = "metadata-only";
                }
                validated[decisionKey] = new ValidatedDefinition(
                    digest, current.Identity.DefinitionId, current.Identity.Revision);
            }
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
        foreach (var definition in bundle.GetProperty("decisions").EnumerateObject())
        {
            var key = definition.Name;
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
                        key,
                        definition.Value,
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

    private static RegisteredDefinitionProjections BuildDefinitionProjections(
        JsonElement bundle,
        string key,
        JsonElement definition,
        string definitionId,
        string revision,
        string contractDigest,
        string bundleDigest)
    {
        var application = bundle.GetProperty("application");
        var evidence = DecisionManifest.ReadEvidence(definition);
        var registeredInputs = DecisionManifest.ReadInputs(definition, evidence);
        var runtimeContext = DecisionManifest.ReadContext(definition);
        var result = DecisionManifest.ReadResult(definition.GetProperty("result"));
        var numberActionSpace = result.ValueType == "number"
            ? new NumberActionSpaceContract(result.Minimum!.Value, result.Maximum!.Value, result.Step)
            : null;
        var policy = ReadPolicy(definition);
        var identity = new RuntimeContractIdentity(
            definitionId,
            contractDigest,
            revision,
            bundleDigest);
        var targeting = definition.GetProperty("targeting");
        var targetHierarchy = targeting.GetProperty("hierarchy").EnumerateArray().Select(value => value.GetString()!).ToArray();
        var inferenceTarget = targeting.GetProperty("primary").GetString()!;
        var fallbackOrder = targeting.GetProperty("fallbackOrder").EnumerateArray().Select(value => value.GetString()!).ToArray();
        var runtime = new RuntimeDecisionDefinition(
            application.GetProperty("id").GetString()!,
            application.GetProperty("environment").GetString()!,
            key,
            identity,
            result.ValueType,
            result.DefaultValue,
            "configured_fallback",
            registeredInputs,
            runtimeContext,
            NumberActionSpace: numberActionSpace,
            Policy: policy,
            TargetHierarchy: targetHierarchy,
            InferenceTarget: inferenceTarget,
            FallbackOrder: fallbackOrder,
            Evidence: evidence);
        var intelligence = new IntelligenceLifecycleDefinitionSnapshot(
            runtime.AppId,
            runtime.Environment,
            runtime.DecisionKey,
            identity,
            runtime.LifecycleStatus,
            DecisionManifest.ReadObjectives(definition),
            evidence,
            result,
            policy ?? new DecisionPolicyContract());
        return new RegisteredDefinitionProjections(runtime, intelligence);
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
        foreach (var definition in bundle.GetProperty("decisions").EnumerateObject())
        {
            var key = definition.Name;
            var digest = CanonicalJson.ContractDigest(key, definition.Value);
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
            ? bundle.GetProperty("decisions")
                .EnumerateObject()
                .Select(
                    definition => JsonSerializer.SerializeToElement(
                        new
                        {
                            kind = "metadata-updated",
                            decisionKey = definition.Name
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
        return bundle.GetProperty("decisions")
            .EnumerateObject()
            .ToDictionary(
                definition => definition.Name,
                definition =>
                {
                    var current = FindCurrent(
                        appId,
                        environment,
                        definition.Name);
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

    private static ProblemIssue Issue(
        string code,
        string path,
        string message,
        string? decisionKey = null,
        string? inputKey = null) =>
        new(code, "error", path, message, decisionKey, inputKey);

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
