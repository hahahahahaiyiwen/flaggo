using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;

namespace Flaggo.ExecutableStore;

public interface IExecutableStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<ExecutableStoreWriteResult> PutCandidateAsync(
        StoredExecutable executable,
        CancellationToken cancellationToken = default);

    Task<AnalysisCandidateStoreResult> PutAnalysisCandidateAsync(
        StoredExecutable executable,
        AnalysisCandidateAdmission admission,
        CancellationToken cancellationToken = default);

    Task<StoredExecutable?> GetAsync(
        string executableDigest,
        CancellationToken cancellationToken = default);

    Task<StoredExecutable?> GetActiveAsync(
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<ExecutablePage> ListCandidatesAsync(
        string contractDigest,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);

    Task<ExecutablePage> ListExecutablesAsync(
        string contractDigest,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);

    Task<ActivationResult> ActivateAsync(
        string contractDigest,
        string executableDigest,
        string? expectedActiveExecutableDigest = null,
        CancellationToken cancellationToken = default);

    Task<ActivationResult> ActivateIfNoneAsync(
        string contractDigest,
        string executableDigest,
        CancellationToken cancellationToken = default);

    Task<CandidateActivationResult> ActivateCandidateAsync(
        CandidateActivationCommand command,
        CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);
}

public sealed record StoredExecutable(
    string ExecutableDigest,
    DecisionExecutable Executable,
    CheckedDecisionExecutable CheckedExecutable,
    ExecutableLifecycleState State,
    long StateVersion,
    DateTimeOffset CreatedAt,
    DateTimeOffset? ActivatedAt,
    JsonElement? Provenance = null);

public enum ExecutableLifecycleState
{
    Candidate,
    Active,
    Inactive
}

public enum ExecutableStoreWriteResult
{
    Created,
    Existing
}

public sealed record AnalysisCandidateAdmission(
    string WorkspaceId,
    string CycleId,
    string AttemptId,
    string ContractName,
    string ContractDigest,
    string ExecutableDigest,
    string AnalysisManifestDigest,
    DateTimeOffset CreatedAt);

public sealed record AnalysisCandidateStoreResult(
    string ExecutableDigest,
    DateTimeOffset CreatedAt,
    ExecutableLifecycleState LifecycleState,
    bool Created);

public sealed record ExecutablePage(
    IReadOnlyList<StoredExecutable> Executables,
    string? NextCursor);

public sealed record ActivationResult(
    string ExecutableDigest,
    string? PreviousExecutableDigest,
    long StateVersion,
    bool Changed);

public sealed record CandidateActivationCommand(
    string ContractName,
    string ContractDigest,
    string ExecutableDigest,
    long ExpectedCandidateStateVersion,
    ExecutableLifecycleToken ExpectedActiveExecutable);

public sealed record CandidateActivationResult(
    string ExecutableDigest,
    string PreviousExecutableDigest,
    long StateVersion,
    int SupersededCandidateCount);

public sealed class ExecutableNotFoundException(
    string executableDigest)
    : Exception($"Executable '{executableDigest}' does not exist.");

public sealed class ExecutableContractMismatchException(
    string executableDigest,
    string expectedContractDigest,
    string actualContractDigest)
    : Exception(
        $"Executable '{executableDigest}' binds contract '{actualContractDigest}', not '{expectedContractDigest}'.");

public sealed class ActivationConflictException(
    string? expectedExecutableDigest,
    string? actualExecutableDigest)
    : Exception(
        $"Active executable changed: expected '{expectedExecutableDigest ?? "<none>"}', "
        + $"found '{actualExecutableDigest ?? "<none>"}'.");

public sealed class CandidateActivationConflictException(
    string executableDigest,
    long expectedStateVersion,
    ExecutableLifecycleState actualState,
    long actualStateVersion)
    : Exception(
        $"Candidate '{executableDigest}' changed: expected candidate state version "
        + $"'{expectedStateVersion}', found '{actualState}' state version "
        + $"'{actualStateVersion}'.");

public sealed class ActivationContractConflictException(
    string contractName,
    string expectedContractDigest,
    string? actualContractDigest)
    : Exception(
        $"Contract '{contractName}' current digest changed: expected "
        + $"'{expectedContractDigest}', found '{actualContractDigest ?? "<none>"}'.");

public sealed class CandidateAdmissionConflictException(
    string workspaceId,
    string cycleId)
    : Exception(
        $"Workspace '{workspaceId}' cycle '{cycleId}' already admitted a different Candidate.")
{
    public string WorkspaceId { get; } = workspaceId;

    public string CycleId { get; } = cycleId;
}

public sealed class CandidateLifecycleConflictException(
    string executableDigest,
    ExecutableLifecycleState state)
    : Exception(
        $"Executable '{executableDigest}' is already in lifecycle state '{state}' and cannot "
        + "be admitted as an inactive Candidate.")
{
    public string ExecutableDigest { get; } = executableDigest;

    public ExecutableLifecycleState State { get; } = state;
}
