using Flaggo.Contract;

namespace Flaggo.ContractStore;

public interface IContractVersionStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<ContractStoreWriteResult> PutAsync(
        AcceptedContractVersion version,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetAsync(
        DecisionScope scope,
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetCurrentAsync(
        DecisionScope scope,
        string contractName,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<AcceptedContractVersion>> ListCurrentAsync(
        DecisionScope scope,
        CancellationToken cancellationToken = default);

    Task<ContractVersionPage> ListAsync(
        DecisionScope scope,
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);

    Task SetCurrentAsync(
        DecisionScope scope,
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);
}

public sealed record AcceptedContractVersion(
    DecisionScope Scope,
    string ContractDigest,
    DateTimeOffset AcceptedAt,
    DecisionContract Contract);

public sealed record ContractVersionPage(
    IReadOnlyList<AcceptedContractVersion> Versions,
    string? NextCursor);

public enum ContractStoreWriteResult
{
    Created,
    Existing
}

public sealed class ContractVersionNotFoundException(
    DecisionScope scope,
    string contractName,
    string contractDigest)
    : Exception(
        $"Contract version '{contractName}' at '{contractDigest}' does not exist in scope '{scope}'.");
