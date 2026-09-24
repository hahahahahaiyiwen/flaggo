using System.Security.Cryptography;
using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Audit;

public sealed record DecisionAuditRecord(
    string AuditId,
    string DecisionId,
    string DecisionKey,
    string AppId,
    string Environment,
    RuntimeContractIdentity Contract,
    JsonElement Value,
    string ValueType,
    string DecisionMode,
    ServerFallbackInfo Fallback,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    IReadOnlyDictionary<string, JsonElement> Inputs,
    DecisionTargetRef? RuntimeTarget,
    DecisionTargetRef? ControlTarget,
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    IReadOnlyList<string> ResolutionChain,
    PolicyEvaluationResult Policy,
    DateTimeOffset RecordedAt,
    string TenantId,
    DecisionEvidenceSnapshot? Evidence = null,
    ConfidenceReport? Confidence = null,
    string? StrategyId = null,
    string? Reason = null,
    IReadOnlyDictionary<string, JsonElement>? RequestInputs = null,
    IReadOnlyDictionary<string, InputProvenance>? InputProvenance = null);

public sealed record ExposureAuditRecord(
    string ExposureId,
    string DecisionId,
    string AppId,
    string Environment,
    string? AppliedAt,
    string ConfirmedAt);

public interface IAuditSink
{
    Task RecordDecisionAsync(DecisionAuditRecord record, CancellationToken cancellationToken);
}

public interface IAuditHealth
{
    Task<bool> IsAvailableAsync(CancellationToken cancellationToken);
}

public interface IExposureAuditSink
{
    Task RecordExposureAsync(
        ExposureAuditRecord record,
        CancellationToken cancellationToken);
}

public sealed class ExposureAuditConflictException(string exposureId) :
    InvalidOperationException(
        $"Exposure audit identity '{exposureId}' is already bound to a " +
        "different record.")
{
    public string ExposureId { get; } = exposureId;
}

public sealed class InMemoryAuditSink(bool available = true) :
    IAuditSink,
    IExposureAuditSink,
    IAuditHealth
{
    private readonly List<DecisionAuditRecord> _records = [];
    private readonly List<ExposureAuditRecord> _exposureRecords = [];
    private readonly Dictionary<string, string> _exposureHashes =
        new(StringComparer.Ordinal);
    private readonly object _gate = new();

    public IReadOnlyList<DecisionAuditRecord> Records
    {
        get
        {
            lock (_gate)
            {
                return _records.ToArray();
            }
        }
    }

    public IReadOnlyList<ExposureAuditRecord> ExposureRecords
    {
        get
        {
            lock (_gate)
            {
                return _exposureRecords.ToArray();
            }
        }
    }

    public Task RecordDecisionAsync(DecisionAuditRecord record, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            _records.Add(record);
        }

        return Task.CompletedTask;
    }

    public Task RecordExposureAsync(
        ExposureAuditRecord record,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var hash = ExposureAuditIdentity.Hash(record);
        lock (_gate)
        {
            if (_exposureHashes.TryGetValue(record.ExposureId, out var existing))
            {
                if (!ExposureAuditIdentity.HashEquals(existing, hash))
                {
                    throw new ExposureAuditConflictException(record.ExposureId);
                }
                return Task.CompletedTask;
            }

            _exposureHashes.Add(record.ExposureId, hash);
            _exposureRecords.Add(record);
        }

        return Task.CompletedTask;
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}

internal static class ExposureAuditIdentity
{
    public static string Hash(ExposureAuditRecord record)
    {
        var element = JsonSerializer.SerializeToElement(
            record,
            new JsonSerializerOptions(JsonSerializerDefaults.Web));
        return Convert.ToHexString(
                SHA256.HashData(CanonicalJson.Canonicalize(element)))
            .ToLowerInvariant();
    }

    public static bool HashEquals(string left, string right) =>
        left.Length == right.Length &&
        CryptographicOperations.FixedTimeEquals(
            Convert.FromHexString(left),
            Convert.FromHexString(right));
}

internal readonly record struct ValidatedAuditRecord(
    string? ExposureId,
    string? ExposureHash);
