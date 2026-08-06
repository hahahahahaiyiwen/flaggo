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
    IReadOnlyList<SignalInput> Inputs,
    DecisionTargetRef? RuntimeTarget,
    DecisionTargetRef? ControlTarget,
    IReadOnlyList<TargetResolutionProvenance> TargetProvenance,
    IReadOnlyList<string> ResolutionChain,
    PolicyEvaluationResult Policy,
    DateTimeOffset RecordedAt,
    DecisionEvidenceSnapshot? Evidence = null,
    ConfidenceReport? Confidence = null,
    string? StrategyId = null);

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

public sealed class InMemoryAuditSink(bool available = true) :
    IAuditSink,
    IExposureAuditSink,
    IAuditHealth
{
    private readonly List<DecisionAuditRecord> _records = [];
    private readonly List<ExposureAuditRecord> _exposureRecords = [];
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
        lock (_gate)
        {
            if (_exposureRecords.All(item =>
                    !string.Equals(
                        item.ExposureId,
                        record.ExposureId,
                        StringComparison.Ordinal)))
            {
                _exposureRecords.Add(record);
            }
        }

        return Task.CompletedTask;
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}
