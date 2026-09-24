using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Flaggo.Shared.Contracts;

namespace Flaggo.Evidence;

public sealed record LocalInputEvidenceStoreOptions(
    string CommitDescriptorPath,
    int MaximumFrames = 10_000,
    int MaximumSnapshotBytes = 16 * 1024 * 1024);

public sealed class LocalInputEvidenceStore : IInputEvidenceSnapshotStore
{
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        PropertyNameCaseInsensitive = false,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
        RespectRequiredConstructorParameters = true,
        RespectNullableAnnotations = true
    };
    private readonly string _descriptor;
    private readonly LocalInputEvidenceStoreOptions _options;
    private FileStream? _lease;
    private string? _artifact;
    private string? _retiredArtifact;
    private bool _disposed;

    public LocalInputEvidenceStore(LocalInputEvidenceStoreOptions options)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(options.CommitDescriptorPath);
        if (options.MaximumFrames <= 0 || options.MaximumSnapshotBytes <= 0)
            throw new ArgumentOutOfRangeException(nameof(options), "Evidence store limits must be positive.");
        _options = options;
        _descriptor = Path.GetFullPath(options.CommitDescriptorPath);
    }

    public async Task<InputEvidenceSnapshot> LoadAsync(CancellationToken cancellationToken)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        cancellationToken.ThrowIfCancellationRequested();
        AcquireLease();
        RemoveRetiredArtifact();
        if (!DescriptorExists())
        {
            var directory = Path.GetDirectoryName(_descriptor)!;
            if (Directory.EnumerateFileSystemEntries(directory).Any(path =>
                    !string.Equals(path, $"{_descriptor}.lock", StringComparison.OrdinalIgnoreCase)))
            {
                throw new InvalidDataException("Cannot initialize evidence storage containing files without a verified commit.");
            }
            var initial = new InputEvidenceSnapshot(1, Guid.NewGuid().ToString("N"), []);
            await PublishAsync(initial, cancellationToken);
            return initial;
        }
        var options = new CommittedFileSnapshotOptions { MaximumArtifactBytes = _options.MaximumSnapshotBytes };
        var reference = await CommittedFileSnapshot.ResolveAsync(
            CommittedFileSnapshotSource.FromDescriptor(_descriptor), options, cancellationToken);
        var bytes = await CommittedFileSnapshot.ReadPinnedAsync(reference, options, cancellationToken);
        StrictJson.Validate(bytes);
        var snapshot = JsonSerializer.Deserialize<InputEvidenceSnapshot>(bytes, JsonOptions)
            ?? throw new InvalidDataException("The materialized input snapshot is empty.");
        Validate(snapshot);
        _artifact = reference.ArtifactPath;
        return snapshot;
    }

    public async Task PublishAsync(InputEvidenceSnapshot snapshot, CancellationToken cancellationToken)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        AcquireLease();
        RemoveRetiredArtifact();
        Validate(snapshot);
        using var buffer = new BoundedSnapshotBuffer(_options.MaximumSnapshotBytes);
        await JsonSerializer.SerializeAsync(buffer, snapshot, JsonOptions, cancellationToken);
        var publication = await CommittedFileSnapshotWriter.PublishAsync(
            _descriptor, buffer.ToArray(), cancellationToken, artifactStem: "input-evidence");
        _retiredArtifact = _artifact;
        _artifact = publication.ArtifactPath;
        RemoveRetiredArtifact();
    }

    private void Validate(InputEvidenceSnapshot snapshot)
    {
        if (snapshot.Version != 1 || !Guid.TryParseExact(snapshot.Generation, "N", out _) ||
            snapshot.Frames is null)
            throw new InvalidDataException("Unsupported or incomplete materialized input snapshot.");
        if (snapshot.Frames.Count > _options.MaximumFrames)
            throw new InputEvidenceCapacityException("Materialized input frame capacity exceeded.");
        var keys = new HashSet<(InputEvidenceKey, string)>();
        foreach (var frame in snapshot.Frames)
        {
            if (frame is null || frame.Key is null || frame.Key.Scope is null || frame.Key.Target is null ||
                string.IsNullOrWhiteSpace(frame.Key.Scope.AppId) || string.IsNullOrWhiteSpace(frame.Key.Scope.Environment) ||
                string.IsNullOrWhiteSpace(frame.Key.Scope.TenantId) ||
                string.IsNullOrWhiteSpace(frame.Key.DefinitionId) || string.IsNullOrWhiteSpace(frame.Key.Revision) ||
                !Digest(frame.Key.ContractDigest) || string.IsNullOrWhiteSpace(frame.Key.Binding) ||
                string.IsNullOrWhiteSpace(frame.Key.Target.Type) || string.IsNullOrWhiteSpace(frame.Key.Target.Id) ||
                frame.Kind is not ("metric" or "span" or "log") ||
                frame.Status is not ("available" or "invalid-value" or "unit-mismatch" or "invalid-duration" or
                    "no-recorded-value" or "unsupported-metric-type" or "invalid-exposure" or "ambiguous") ||
                !ulong.TryParse(frame.TimeUnixNano, NumberStyles.None, CultureInfo.InvariantCulture, out var timestamp) ||
                timestamp == 0 || frame.MaxAgeSeconds is <= 0 or > 922337203685 ||
                !Digest(frame.Fingerprint) || (frame.Kind == "metric" ? !Digest(frame.Stream) : frame.Stream != "") ||
                !keys.Add((frame.Key, frame.Stream)) ||
                (frame.Status == "available" && (frame.Value is null || !DecisionValues.IsScalar(frame.Value.Value))))
            {
                throw new InvalidDataException("The materialized input snapshot contains an invalid or duplicate frame.");
            }
        }
    }

    private static bool Digest(string? value) => value is { Length: 71 } &&
        value.StartsWith("sha256:", StringComparison.Ordinal) &&
        value.AsSpan(7).IndexOfAnyExcept("0123456789abcdef") < 0;

    private bool DescriptorExists()
    {
        try
        {
            _ = File.GetAttributes(_descriptor);
            return true;
        }
        catch (FileNotFoundException) { return false; }
    }

    private void AcquireLease()
    {
        if (_lease is not null) return;
        Directory.CreateDirectory(Path.GetDirectoryName(_descriptor)!);
        _lease = new FileStream($"{_descriptor}.lock", FileMode.OpenOrCreate, FileAccess.ReadWrite,
            FileShare.None, 1, FileOptions.Asynchronous);
    }

    private void RemoveRetiredArtifact()
    {
        if (_retiredArtifact is null) return;
        File.Delete(_retiredArtifact);
        _retiredArtifact = null;
    }

    public async ValueTask DisposeAsync()
    {
        if (_disposed) return;
        _disposed = true;
        if (_lease is not null) await _lease.DisposeAsync();
    }

    private sealed class BoundedSnapshotBuffer(int limit) : MemoryStream
    {
        private void RequireCapacity(int count)
        {
            if (Position > limit - count)
                throw new InputEvidenceCapacityException("Materialized input snapshot byte capacity exceeded.");
        }

        public override void Write(byte[] buffer, int offset, int count)
        {
            RequireCapacity(count);
            base.Write(buffer, offset, count);
        }

        public override void Write(ReadOnlySpan<byte> buffer)
        {
            RequireCapacity(buffer.Length);
            base.Write(buffer);
        }

        public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
        {
            cancellationToken.ThrowIfCancellationRequested();
            Write(buffer.Span);
            return ValueTask.CompletedTask;
        }
    }
}
