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
public sealed record SignalRef(string Key);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record SignalInput(SignalRef Signal, JsonElement Value);

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed record ProblemIssue(
    string Code,
    string Severity,
    string Path,
    string Message,
    string? DecisionKey = null,
    string? SignalKey = null);

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
    IReadOnlyList<SignalInput>? Inputs = null);

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

public sealed record DecisionFailure(
    int Status,
    string Code,
    string Detail,
    IReadOnlyList<ProblemIssue>? Issues = null,
    ClientFallbackEligibility? ClientFallback = null);

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
