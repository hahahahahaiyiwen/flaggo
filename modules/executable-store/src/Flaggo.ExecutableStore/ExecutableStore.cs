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

    Task<StoredExecutable?> GetAsync(
        DecisionScope scope,
        string executableDigest,
        CancellationToken cancellationToken = default);

    Task<StoredExecutable?> GetActiveAsync(
        DecisionScope scope,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<ActivationResult> ActivateAsync(
        DecisionScope scope,
        string contractDigest,
        string executableDigest,
        string? expectedActiveExecutableDigest = null,
        CancellationToken cancellationToken = default);

    Task<ActivationResult> ActivateIfNoneAsync(
        DecisionScope scope,
        string contractDigest,
        string executableDigest,
        CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);
}

public sealed record StoredExecutable(
    DecisionScope Scope,
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

public sealed record ActivationResult(
    string ExecutableDigest,
    string? PreviousExecutableDigest,
    long StateVersion,
    bool Changed);

public sealed class ExecutableNotFoundException(
    DecisionScope scope,
    string executableDigest)
    : Exception($"Executable '{executableDigest}' does not exist in scope '{scope}'.");

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
