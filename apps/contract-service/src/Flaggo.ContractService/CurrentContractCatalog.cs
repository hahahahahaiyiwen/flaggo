using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Serialization;
using Flaggo.Contract;
using Flaggo.ContractStore;

namespace Flaggo.ContractService;

public sealed record CurrentContractCatalog
{
    [JsonPropertyName("contracts")]
    public required IReadOnlyList<CurrentContractCatalogEntry> Contracts { get; init; }
}

public sealed record CurrentContractCatalogEntry
{
    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("contract")]
    public required DecisionContract Contract { get; init; }
}

public sealed record CurrentContractCatalogResult(
    CurrentContractCatalog Catalog,
    string Etag);

internal static class CurrentContractCatalogs
{
    private static readonly byte[] Domain =
        "flaggo-current-contract-catalog-v1"u8.ToArray();

    public static CurrentContractCatalogResult Project(
        IReadOnlyList<AcceptedContractVersion> currentVersions)
    {
        var contracts = currentVersions
            .OrderBy(version => version.Contract.Authority.Tenant, StringComparer.Ordinal)
            .ThenBy(version => version.Contract.Authority.Application, StringComparer.Ordinal)
            .ThenBy(version => version.Contract.Authority.Environment, StringComparer.Ordinal)
            .ThenBy(version => version.Contract.Name, StringComparer.Ordinal)
            .ThenBy(version => version.ContractDigest, StringComparer.Ordinal)
            .Select(version => new CurrentContractCatalogEntry
            {
                ContractDigest = version.ContractDigest,
                Contract = version.Contract
            })
            .ToArray();

        return new CurrentContractCatalogResult(
            new CurrentContractCatalog { Contracts = contracts },
            ComputeEtag(contracts));
    }

    private static string ComputeEtag(
        IReadOnlyList<CurrentContractCatalogEntry> contracts)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        hash.AppendData(Domain);
        foreach (var contract in contracts)
        {
            hash.AppendData(Encoding.UTF8.GetBytes(contract.ContractDigest));
        }

        return $"\"catalog:{Convert.ToHexStringLower(hash.GetHashAndReset())}\"";
    }
}
