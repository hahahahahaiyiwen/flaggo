using Flaggo.Contract;

namespace Flaggo.ContractStore;

public interface IContractVersionStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<ContractStoreWriteResult> PutAsync(
        AcceptedContractVersion version,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetAsync(
        AuthorityScope scope,
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetCurrentAsync(
        AuthorityScope scope,
        string contractName,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<AcceptedContractVersion>> ListCurrentAsync(
        AuthorityScope scope,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<AcceptedContractVersion>> ListAllCurrentAsync(
        CancellationToken cancellationToken = default);

    Task<ContractVersionPage> ListAsync(
        AuthorityScope scope,
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);

    Task SetCurrentAsync(
        AuthorityScope scope,
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);
}

public sealed record AcceptedContractVersion(
    AuthorityScope Scope,
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
    AuthorityScope scope,
    string contractName,
    string contractDigest)
    : Exception(
        $"Contract version '{contractName}' at '{contractDigest}' does not exist in scope '{scope}'.");
