using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Serialization;
using Flaggo.Contract;
using Flaggo.ContractStore;

namespace Flaggo.ContractService;

public sealed record ContractSelectorSnapshot
{
    [JsonPropertyName("snapshotDigest")]
    public required string SnapshotDigest { get; init; }

    [JsonPropertyName("contracts")]
    public required IReadOnlyList<ContractSelectorSnapshotEntry> Contracts { get; init; }
}

public sealed record ContractSelectorSnapshotEntry
{
    [JsonPropertyName("name")]
    public required string Name { get; init; }

    [JsonPropertyName("contractDigest")]
    public required string ContractDigest { get; init; }

    [JsonPropertyName("contract")]
    public required DecisionContract Contract { get; init; }
}

internal static class ContractSelectorSnapshots
{
    private static readonly byte[] Domain =
        "flaggo-selector-snapshot-v1"u8.ToArray();

    public static ContractSelectorSnapshot Project(
        IReadOnlyList<AcceptedContractVersion> currentVersions)
    {
        var contracts = currentVersions
            .Where(version => version.Contract.Learning?.Evidence.Count > 0)
            .OrderBy(version => version.Contract.Name, StringComparer.Ordinal)
            .ThenBy(version => version.ContractDigest, StringComparer.Ordinal)
            .Select(version => new ContractSelectorSnapshotEntry
            {
                Name = version.Contract.Name,
                ContractDigest = version.ContractDigest,
                Contract = version.Contract
            })
            .ToArray();

        return new ContractSelectorSnapshot
        {
            SnapshotDigest = ComputeDigest(contracts),
            Contracts = contracts
        };
    }

    private static string ComputeDigest(
        IReadOnlyList<ContractSelectorSnapshotEntry> contracts)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        Append(hash, Domain);
        foreach (var contract in contracts)
        {
            Append(hash, Encoding.UTF8.GetBytes(contract.Name));
            Append(hash, Encoding.UTF8.GetBytes(contract.ContractDigest));
        }

        return $"sha256:{Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant()}";
    }

    private static void Append(IncrementalHash hash, ReadOnlySpan<byte> value)
    {
        Span<byte> length = stackalloc byte[sizeof(ulong)];
        BinaryPrimitives.WriteUInt64BigEndian(length, checked((ulong)value.Length));
        hash.AppendData(length);
        hash.AppendData(value);
    }
}
