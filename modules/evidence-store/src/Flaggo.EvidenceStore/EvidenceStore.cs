using System.Text.Json;
using Flaggo.Contract;

namespace Flaggo.EvidenceStore;

public enum EvidenceObservationWriteResult
{
    Created,
    Existing
}

public sealed record DecisionObservation(
    DecisionScope Scope,
    string DecisionId,
    string ContractName,
    string ContractDigest,
    string ExecutableDigest,
    JsonElement Result,
    string ResultHash,
    string EvaluationSource,
    string? EvaluationRule,
    IReadOnlyDictionary<string, JsonElement> CorrelationAttributes,
    DateTimeOffset ObservedAt);

public sealed record OutcomeObservation(
    DecisionScope Scope,
    string ObservationId,
    string Binding,
    JsonElement Value,
    string? DecisionId,
    string? ContractName,
    string? ContractDigest,
    IReadOnlyDictionary<string, JsonElement> CorrelationAttributes,
    DateTimeOffset ObservedAt);

public interface IEvidenceStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);

    Task<EvidenceObservationWriteResult> PutDecisionAsync(
        DecisionObservation observation,
        CancellationToken cancellationToken = default);

    Task<EvidenceObservationWriteResult> PutOutcomeAsync(
        OutcomeObservation observation,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<DecisionObservation>> ListDecisionsAsync(
        DecisionScope scope,
        int limit,
        string? contractName = null,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<OutcomeObservation>> ListOutcomesAsync(
        DecisionScope scope,
        int limit,
        string? binding = null,
        CancellationToken cancellationToken = default);
}
