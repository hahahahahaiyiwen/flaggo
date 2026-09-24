using System.Text.Json;
using System.Text.Json.Serialization;

namespace Flaggo.Shared.Contracts;

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record RuntimeContractIdentity(
    string DefinitionId,
    string ContractDigest,
    string Revision,
    string? BundleDigest = null,
    string? BuildId = null,
    string? DeploymentId = null,
    string? ArtifactDigest = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionTargetRef(string Type, string Id);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionClient(
    string AppId,
    string Environment,
    string? Sdk = null,
    string? SdkVersion = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ApplicationScope(string AppId, string Environment, string TenantId);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record InputProvenance(
    string Source,
    string? Binding = null,
    string? Generation = null,
    string? ObservedTimeUnixNano = null,
    DateTimeOffset? MaterializedAt = null,
    string? Coverage = null,
    string? SourceFingerprint = null,
    string? TraceId = null,
    string? SpanId = null,
    uint? SamplingFlags = null,
    string? ExposureId = null);

public static class DecisionValues
{
    public static bool IsScalar(JsonElement value) =>
        value.ValueKind is JsonValueKind.String or JsonValueKind.True or JsonValueKind.False ||
        CanonicalJson.IsIeee754CompatibleNumber(value);

    public static bool Matches(JsonElement value, string type, double? minimum = null, double? maximum = null) =>
        type switch
        {
            "number" => CanonicalJson.IsIeee754CompatibleNumber(value) &&
                (minimum is null || value.GetDouble() >= minimum) &&
                (maximum is null || value.GetDouble() <= maximum),
            "boolean" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "string" => value.ValueKind == JsonValueKind.String,
            _ => false
        };
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ProblemIssue(
    string Code,
    string Severity,
    string Path,
    string Message,
    string? DecisionKey = null,
    string? InputKey = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ClientFallbackEligibility(
    bool Eligible,
    string? Reason = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecideRequest(
    RuntimeContractIdentity ExpectedContract,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    DecisionClient Client,
    DecisionTargetRef? RuntimeTarget = null,
    IReadOnlyDictionary<string, JsonElement>? Inputs = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionDefinitionRef(
    string AppId,
    string Environment,
    string Key,
    string DefinitionId,
    string Revision);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record TargetResolutionProvenance(
    string TargetType,
    string ResolvedId,
    string Source,
    string? ClaimedId = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ConfidenceReport(
    double EvidenceQuality,
    double? ModelUncertainty = null,
    double? ExpectedOutcome = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionEvidenceSnapshot(
    double EvidenceQuality,
    double? ModelUncertainty = null,
    double? ExpectedOutcome = null,
    double? SampleSize = null,
    IReadOnlyDictionary<string, JsonElement>? Details = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ServerFallbackInfo(
    string Source,
    bool ResolutionFallbackUsed,
    bool DecisionFallbackUsed,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.Never)] string? Reason);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record PolicyEvaluationResult(
    string Result,
    IReadOnlyList<string> Reasons,
    IReadOnlyList<string> AppliedConstraints);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ContractRuntimeStatus(
    string DefinitionId,
    string Revision,
    string ContractDigest,
    string Integrity,
    string? BundleDigest = null,
    string? BuildId = null,
    string? DeploymentId = null,
    string? Compatibility = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ExposureDirective(
    bool ConfirmationRequired,
    string? ConfirmToken = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ServerDecisionResult(
    string DecisionKey,
    DecisionDefinitionRef Definition,
    string DecisionId,
    JsonElement Value,
    string ValueType,
    string DecisionMode,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.Never)] ConfidenceReport? Confidence,
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    IReadOnlyList<string> ResolutionChain,
    ServerFallbackInfo Fallback,
    PolicyEvaluationResult Policy,
    ContractRuntimeStatus DefinitionStatus,
    ExposureDirective Exposure,
    string Reason,
    string AuditId,
    DecisionTargetRef? RuntimeTarget = null,
    DecisionTargetRef? ControlTarget = null,
    string? StrategyId = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record FlaggoProblem(
    string Type,
    int Status,
    string Code,
    string? Title = null,
    string? Detail = null,
    string? Instance = null,
    string? CorrelationId = null,
    IReadOnlyList<ProblemIssue>? Issues = null,
    int? RetryAfterSeconds = null,
    ClientFallbackEligibility? ClientFallback = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record DecisionFailure(
    int Status,
    string Code,
    string Detail,
    IReadOnlyList<ProblemIssue>? Issues = null,
    ClientFallbackEligibility? ClientFallback = null,
    int? RetryAfterSeconds = null);

public sealed record DecideTerminalOutcome(
    ServerDecisionResult? Result,
    DecisionFailure? Failure)
{
    public static DecideTerminalOutcome Success(ServerDecisionResult result) => new(result, null);

    public static DecideTerminalOutcome Rejected(DecisionFailure failure) => new(null, failure);
}

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ExposureConfirmationRequest(
    string ConfirmToken,
    string? AppliedAt = null);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ExposureConfirmationResult(
    string ExposureId,
    string DecisionId,
    string Status,
    string ConfirmedAt);

[JsonSerializable(typeof(DecideRequest))]
[JsonSerializable(typeof(ServerDecisionResult))]
[JsonSerializable(typeof(FlaggoProblem))]
[JsonSerializable(typeof(ProblemIssue))]
[JsonSerializable(typeof(ExposureConfirmationRequest))]
[JsonSerializable(typeof(ExposureConfirmationResult))]
public partial class RuntimeJsonContext : JsonSerializerContext;
