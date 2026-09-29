using System.Text.Json;
using Flaggo.Contract;

namespace Flaggo.EvidenceStore;

public sealed record EvidenceTelemetryRecord(
    DecisionScope Scope,
    string ObservationId,
    string Signal,
    JsonElement Payload,
    DateTimeOffset ObservedAt,
    DateTimeOffset ReceivedAt);

public sealed record EvidenceTelemetryWriteResult(
    int Created,
    int Existing);

public interface IEvidenceStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);

    Task<EvidenceTelemetryWriteResult> PutTelemetryBatchAsync(
        IReadOnlyList<EvidenceTelemetryRecord> records,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<EvidenceTelemetryRecord>> ListTelemetryAsync(
        DecisionScope scope,
        int limit,
        string? signal = null,
        CancellationToken cancellationToken = default);
}
