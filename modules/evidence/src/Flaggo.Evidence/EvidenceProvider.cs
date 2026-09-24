using System.Text.Json;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Evidence;

public sealed record DecisionEvidenceRequest(
    RuntimeDecisionDefinition Definition,
    GovernedDecisionState State,
    IReadOnlyDictionary<string, JsonElement> RuntimeContext,
    IReadOnlyDictionary<string, JsonElement> Inputs);

public interface IEvidenceProvider
{
    Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
        DecisionEvidenceRequest request,
        CancellationToken cancellationToken);
}

public sealed class EvidenceUnavailableException(
    string message,
    Exception innerException) : Exception(message, innerException);

public interface IEvidenceHealth
{
    Task<bool> IsAvailableAsync(CancellationToken cancellationToken);
}

public sealed class InMemoryEvidenceProvider(
    IReadOnlyDictionary<string, DecisionEvidenceSnapshot>? evidenceByStrategy = null,
    bool available = true)
    : IEvidenceProvider, IEvidenceHealth
{
    private readonly IReadOnlyDictionary<string, DecisionEvidenceSnapshot> _evidenceByStrategy =
        evidenceByStrategy ??
        new Dictionary<string, DecisionEvidenceSnapshot>(StringComparer.Ordinal);

    public Task<DecisionEvidenceSnapshot?> GetEvidenceAsync(
        DecisionEvidenceRequest request,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(
            available &&
            request.State.StrategyId is not null &&
            _evidenceByStrategy.TryGetValue(request.State.StrategyId, out var evidence)
                ? evidence
                : null);
    }

    public Task<bool> IsAvailableAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(available);
    }
}
