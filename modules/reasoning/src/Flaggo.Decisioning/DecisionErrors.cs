using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning;

public sealed class DecisionContractException(
    int status,
    string code,
    string message,
    IReadOnlyList<ProblemIssue>? issues = null,
    ClientFallbackEligibility? clientFallback = null,
    int? retryAfterSeconds = null) : Exception(message)
{
    public int Status { get; } = status;

    public string Code { get; } = code;

    public IReadOnlyList<ProblemIssue>? Issues { get; } = issues;

    public ClientFallbackEligibility? ClientFallback { get; } = clientFallback;

    public int? RetryAfterSeconds { get; } = retryAfterSeconds;
}
