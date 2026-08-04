using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning;

public sealed class DecisionContractException(
    int status,
    string code,
    string message,
    IReadOnlyList<ProblemIssue>? issues = null) : Exception(message)
{
    public int Status { get; } = status;

    public string Code { get; } = code;

    public IReadOnlyList<ProblemIssue>? Issues { get; } = issues;
}
