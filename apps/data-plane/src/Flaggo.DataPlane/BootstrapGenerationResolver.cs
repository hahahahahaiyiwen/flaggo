using Flaggo.Shared.Contracts;
using Flaggo.Evidence;
using Flaggo.State;

namespace Flaggo.DataPlane;

public sealed record BootstrapGenerationPaths(
    string RootPath,
    string Generation,
    string ReceiptPath,
    string StatePath,
    string EvidencePath,
    CommittedArtifactReference ReceiptSnapshot,
    CommittedArtifactReference StateSnapshot,
    CommittedArtifactReference EvidenceSnapshot);

public sealed class BootstrapGenerationResolver :
    IStateSnapshotProvider,
    IEvidenceSnapshotProvider
{
    private static readonly IReadOnlyDictionary<string, string>
        RequiredArtifacts = new Dictionary<string, string>(
            StringComparer.Ordinal)
        {
            ["receipt"] = "receipt.json",
            ["state"] = "state.json",
            ["evidence"] = "evidence.json"
        };

    private readonly string _rootPath;
    private readonly string _manifestPath;
    private readonly Lazy<Task<BootstrapGenerationPaths>> _snapshot;

    public BootstrapGenerationResolver(string rootPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(rootPath);
        _rootPath = Path.GetFullPath(rootPath);
        _manifestPath = Path.Combine(_rootPath, "current.json");
        _snapshot = new Lazy<Task<BootstrapGenerationPaths>>(
            LoadAsync,
            LazyThreadSafetyMode.ExecutionAndPublication);
    }

    public static BootstrapGenerationPaths Resolve(string rootPath)
        => new BootstrapGenerationResolver(rootPath)
            .ResolveAsync(CancellationToken.None)
            .GetAwaiter()
            .GetResult();

    public async Task<BootstrapGenerationPaths> ResolveAsync(
        CancellationToken cancellationToken) =>
        await _snapshot.Value.WaitAsync(cancellationToken);

    public async Task<CommittedArtifactReference> ResolveStateSnapshotAsync(
        CancellationToken cancellationToken) =>
        (await ResolveAsync(cancellationToken)).StateSnapshot;

    public async Task<CommittedArtifactReference> ResolveEvidenceSnapshotAsync(
        CancellationToken cancellationToken) =>
        (await ResolveAsync(cancellationToken)).EvidenceSnapshot;

    private async Task<BootstrapGenerationPaths> LoadAsync()
    {
        var generation =
            await CommittedFileSnapshot.ResolveGenerationManifestAsync(
                _manifestPath,
                RequiredArtifacts,
                CancellationToken.None);
        foreach (var artifact in generation.Artifacts.Values)
        {
            await CommittedFileSnapshot.ReadPinnedAsync(
                    artifact,
                    options: null,
                    CancellationToken.None);
        }

        return new BootstrapGenerationPaths(
            _rootPath,
            generation.Generation,
            generation.Artifacts["receipt"].ArtifactPath,
            generation.Artifacts["state"].ArtifactPath,
            generation.Artifacts["evidence"].ArtifactPath,
            generation.Artifacts["receipt"],
            generation.Artifacts["state"],
            generation.Artifacts["evidence"]);
    }
}

internal sealed class DirectCommittedSnapshotResolver :
    IStateSnapshotProvider,
    IEvidenceSnapshotProvider
{
    private readonly Lazy<Task<CommittedArtifactReference>>? _state;
    private readonly Lazy<Task<CommittedArtifactReference>>? _evidence;

    public DirectCommittedSnapshotResolver(
        string? stateDescriptorPath,
        string? evidenceDescriptorPath)
    {
        _state = CreateSnapshot(stateDescriptorPath);
        _evidence = CreateSnapshot(evidenceDescriptorPath);
    }

    public async Task<CommittedArtifactReference> ResolveStateSnapshotAsync(
        CancellationToken cancellationToken) =>
        await ResolveAsync(
            _state,
            "A local state commit descriptor is not configured.",
            cancellationToken);

    public async Task<CommittedArtifactReference> ResolveEvidenceSnapshotAsync(
        CancellationToken cancellationToken) =>
        await ResolveAsync(
            _evidence,
            "A local evidence commit descriptor is not configured.",
            cancellationToken);

    private static Lazy<Task<CommittedArtifactReference>>? CreateSnapshot(
        string? descriptorPath) =>
        string.IsNullOrWhiteSpace(descriptorPath)
            ? null
            : new Lazy<Task<CommittedArtifactReference>>(
                () => CommittedFileSnapshot.ResolveAsync(
                    CommittedFileSnapshotSource.FromDescriptor(descriptorPath),
                    options: null,
                    CancellationToken.None),
                LazyThreadSafetyMode.ExecutionAndPublication);

    private static async Task<CommittedArtifactReference> ResolveAsync(
        Lazy<Task<CommittedArtifactReference>>? snapshot,
        string missingMessage,
        CancellationToken cancellationToken)
    {
        if (snapshot is null)
        {
            throw new InvalidOperationException(missingMessage);
        }

        return await snapshot.Value.WaitAsync(cancellationToken);
    }
}
