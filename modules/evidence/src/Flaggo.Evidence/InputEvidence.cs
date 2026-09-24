using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;

namespace Flaggo.Evidence;

public sealed record TelemetryEvent(
    string Name,
    ulong TimeUnixNano,
    IReadOnlyDictionary<string, JsonElement> Attributes);

public sealed record TelemetryObservation(
    string Kind,
    string? Name,
    string ScopeName,
    string ScopeVersion,
    IReadOnlyDictionary<string, JsonElement> ResourceAttributes,
    IReadOnlyDictionary<string, JsonElement> Attributes,
    ulong TimeUnixNano,
    string Fingerprint,
    string? MetricStreamFingerprint = null,
    string? DataType = null,
    string? Unit = null,
    JsonElement? Value = null,
    JsonElement? Body = null,
    string? EventName = null,
    ulong? StartTimeUnixNano = null,
    IReadOnlyList<TelemetryEvent>? Events = null,
    string? TraceId = null,
    string? SpanId = null,
    uint? SamplingFlags = null,
    bool NoRecordedValue = false);

public sealed record EvidenceInputRequest(
    string InputKey,
    RegisteredEvidenceBinding Binding,
    DecisionTargetRef Target);

public sealed record InputEvidenceRequest(
    ApplicationScope Scope,
    RuntimeContractIdentity Definition,
    IReadOnlyList<EvidenceInputRequest> Inputs,
    DateTimeOffset EvaluatedAt);

public sealed record InputEvidenceValue(
    JsonElement? Value,
    string Status,
    InputProvenance Provenance);

public sealed record InputEvidenceResult(
    string Generation,
    IReadOnlyDictionary<string, InputEvidenceValue> Inputs);

public interface IInputEvidenceReader
{
    Task<InputEvidenceResult> ReadInputsAsync(
        InputEvidenceRequest request,
        CancellationToken cancellationToken);
}

public sealed record TelemetryIngestResult(
    int AcceptedRecords,
    int RejectedRecords,
    int UnmatchedRecords,
    IReadOnlyList<string> Diagnostics);

public interface IInputTelemetrySink
{
    Task<TelemetryIngestResult> IngestAsync(
        ApplicationScope scope,
        IReadOnlyList<TelemetryObservation> observations,
        CancellationToken cancellationToken);
}

public sealed class InputEvidenceUnavailableException(string message, Exception? inner = null)
    : Exception(message, inner);

public sealed class InputEvidenceCapacityException(string message) : Exception(message);

public sealed record InputEvidenceKey(
    ApplicationScope Scope,
    string DefinitionId,
    string Revision,
    string ContractDigest,
    string Binding,
    DecisionTargetRef Target);

public sealed record InputEvidenceFrame(
    InputEvidenceKey Key,
    string Stream,
    string Kind,
    JsonElement? Value,
    string Status,
    string TimeUnixNano,
    long MaxAgeSeconds,
    DateTimeOffset MaterializedAt,
    string Fingerprint,
    string? TraceId,
    string? SpanId,
    uint? SamplingFlags,
    string? ExposureId);

public sealed record InputEvidenceSnapshot(
    int Version,
    string Generation,
    IReadOnlyList<InputEvidenceFrame> Frames);

public interface IInputEvidenceSnapshotStore : IAsyncDisposable
{
    Task<InputEvidenceSnapshot> LoadAsync(CancellationToken cancellationToken);

    Task PublishAsync(InputEvidenceSnapshot snapshot, CancellationToken cancellationToken);
}
