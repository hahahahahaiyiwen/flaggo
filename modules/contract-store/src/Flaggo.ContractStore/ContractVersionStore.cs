using Flaggo.Contract;

namespace Flaggo.ContractStore;

public interface IContractVersionStore
{
    Task InitializeAsync(CancellationToken cancellationToken = default);

    Task<ContractStoreWriteResult> PutAsync(
        AcceptedContractVersion version,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<AcceptedContractVersion?> GetCurrentAsync(
        string contractName,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<AcceptedContractVersion>> ListAllCurrentAsync(
        CancellationToken cancellationToken = default);

    Task<ContractVersionPage> ListAsync(
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);

    Task SetCurrentAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default);
}

public sealed record AcceptedContractVersion(
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
    string contractName,
    string contractDigest)
    : Exception(
        $"Contract version '{contractName}' at '{contractDigest}' does not exist.");

public sealed class ContractNameAuthorityConflictException(
    string contractName,
    AuthorityScope existingAuthority,
    AuthorityScope requestedAuthority)
    : Exception(
        $"Contract name '{contractName}' belongs to authority '{existingAuthority}', "
        + $"not '{requestedAuthority}'.")
{
    public string ContractName { get; } = contractName;

    public AuthorityScope ExistingAuthority { get; } = existingAuthority;

    public AuthorityScope RequestedAuthority { get; } = requestedAuthority;
}
