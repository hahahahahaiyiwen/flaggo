namespace Flaggo.ExecutableStore;

public interface IAnalysisCandidateActivationStore
{
    Task<AnalysisCandidateActivationPage> ListPendingAsync(
        int pageSize,
        string? afterContractName = null,
        CancellationToken cancellationToken = default);

    Task<AnalysisCandidateResolutionResult> ResolveAsync(
        AnalysisCandidateResolutionCommand command,
        CancellationToken cancellationToken = default);
}

public sealed record ExecutableLifecycleToken(
    string ExecutableDigest,
    long StateVersion);

public sealed record AnalysisCandidateActivationSnapshot(
    AnalysisCandidateAdmission Admission,
    long CandidateStateVersion,
    ExecutableLifecycleToken? ActiveExecutable);

public sealed record AnalysisCandidateActivationPage(
    IReadOnlyList<AnalysisCandidateActivationSnapshot> Candidates,
    string? NextContractName);

public enum AnalysisCandidateResolutionAction
{
    Activate,
    Reject
}

public sealed record AnalysisCandidateResolutionCommand(
    AnalysisCandidateAdmission Admission,
    long ExpectedCandidateStateVersion,
    ExecutableLifecycleToken? ExpectedActiveExecutable,
    AnalysisCandidateResolutionAction Action);

public enum AnalysisCandidateResolutionOutcome
{
    Activated,
    Rejected,
    Superseded,
    Yielded,
    AlreadyResolved
}

public sealed record AnalysisCandidateResolutionResult(
    AnalysisCandidateResolutionOutcome Outcome,
    string ExecutableDigest,
    string? PreviousExecutableDigest,
    long? StateVersion,
    int SupersededCandidateCount);
