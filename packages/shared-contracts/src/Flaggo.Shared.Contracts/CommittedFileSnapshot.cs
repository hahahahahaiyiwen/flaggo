using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace Flaggo.Shared.Contracts;

public sealed record CommittedFileSnapshotOptions
{
    public int MaximumArtifactBytes { get; init; } = 16 * 1024 * 1024;

    public int MaximumCommitBytes { get; init; } = 64 * 1024;

    public int MaximumGenerationSwitchRetries { get; init; } = 2;

    internal ICommittedFileSnapshotObserver? Observer { get; init; }
}

public sealed class CommittedFileSnapshotSource
{
    private CommittedFileSnapshotSource(
        string commitPath,
        string? logicalName,
        IReadOnlyDictionary<string, string>? requiredArtifacts)
    {
        CommitPath = Path.GetFullPath(commitPath);
        LogicalName = logicalName;
        RequiredArtifacts = requiredArtifacts;
    }

    internal string CommitPath { get; }

    internal string? LogicalName { get; }

    internal IReadOnlyDictionary<string, string>? RequiredArtifacts { get; }

    internal bool IsGenerationManifest => LogicalName is not null;

    public static CommittedFileSnapshotSource FromDescriptor(
        string commitDescriptorPath)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(commitDescriptorPath);
        return new CommittedFileSnapshotSource(
            commitDescriptorPath,
            logicalName: null,
            requiredArtifacts: null);
    }

    public static CommittedFileSnapshotSource FromGenerationManifest(
        string manifestPath,
        string logicalName,
        IReadOnlyDictionary<string, string> requiredArtifacts)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(manifestPath);
        ArgumentException.ThrowIfNullOrWhiteSpace(logicalName);
        ArgumentNullException.ThrowIfNull(requiredArtifacts);
        if (requiredArtifacts.Count == 0 ||
            !requiredArtifacts.ContainsKey(logicalName))
        {
            throw new ArgumentException(
                "The generation source must require its logical artifact.",
                nameof(requiredArtifacts));
        }

        return new CommittedFileSnapshotSource(
            manifestPath,
            logicalName,
            new Dictionary<string, string>(
                requiredArtifacts,
                StringComparer.Ordinal));
    }
}

public sealed record CommittedArtifactReference(
    string ArtifactPath,
    long ByteLength,
    string Sha256);

public sealed record CommittedGenerationSnapshot(
    string Generation,
    IReadOnlyDictionary<string, CommittedArtifactReference> Artifacts);

public sealed record CommittedArtifactPublication(
    string CommitDescriptorPath,
    string ArtifactPath,
    long ByteLength,
    string Sha256);

public static partial class CommittedFileSnapshot
{
    public const string ArtifactDescriptorFormat =
        "flaggo.committed-artifact";

    public const string GenerationManifestFormat =
        "flaggo.committed-generation";

    private const int FormatVersion = 1;

    private static readonly JsonSerializerOptions JsonOptions =
        new(JsonSerializerDefaults.Web)
        {
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
        };

    public static async Task<byte[]> ReadAsync(
        CommittedFileSnapshotSource source,
        CommittedFileSnapshotOptions? options,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(source);
        options ??= new CommittedFileSnapshotOptions();
        ValidateOptions(options);

        for (var attempt = 1;
             attempt <= options.MaximumGenerationSwitchRetries + 1;
             attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var commitBytes = await ReadBoundedFileAsync(
                source.CommitPath,
                options.MaximumCommitBytes,
                cancellationToken);
            var artifact = ResolveArtifact(source, commitBytes);
            if (artifact.ByteLength > options.MaximumArtifactBytes)
            {
                throw new InvalidDataException(
                    $"The committed artifact exceeds the " +
                    $"{options.MaximumArtifactBytes}-byte limit.");
            }

            if (options.Observer is not null)
            {
                await options.Observer.AfterCommitReadAsync(
                    attempt,
                    artifact,
                    cancellationToken);
            }

            byte[]? artifactBytes = null;
            Exception? artifactError = null;
            try
            {
                artifactBytes = await ReadExactArtifactAsync(
                    artifact,
                    cancellationToken);
            }
            catch (Exception error) when (
                error is IOException or
                UnauthorizedAccessException)
            {
                artifactError = error;
            }

            if (options.Observer is not null)
            {
                await options.Observer.AfterArtifactReadAsync(
                    attempt,
                    artifact,
                    cancellationToken);
            }

            var currentCommitBytes = await ReadBoundedFileAsync(
                source.CommitPath,
                options.MaximumCommitBytes,
                cancellationToken);
            if (!commitBytes.AsSpan().SequenceEqual(currentCommitBytes))
            {
                if (attempt <= options.MaximumGenerationSwitchRetries)
                {
                    continue;
                }

                throw new IOException(
                    $"The commit descriptor '{source.CommitPath}' changed " +
                    "during every bounded read attempt.");
            }

            if (artifactError is not null)
            {
                throw artifactError;
            }

            VerifyArtifact(artifact, artifactBytes!);
            return artifactBytes!;
        }

        throw new InvalidOperationException(
            "The committed snapshot retry loop terminated unexpectedly.");
    }

    public static async Task<byte[]> ReadPinnedAsync(
        CommittedArtifactReference artifact,
        CommittedFileSnapshotOptions? options,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(artifact);
        options ??= new CommittedFileSnapshotOptions();
        ValidateOptions(options);
        if (artifact.ByteLength > options.MaximumArtifactBytes)
        {
            throw new InvalidDataException(
                $"The committed artifact exceeds the " +
                $"{options.MaximumArtifactBytes}-byte limit.");
        }

        var bytes = await ReadExactArtifactAsync(
            artifact,
            cancellationToken);
        VerifyArtifact(artifact, bytes);
        return bytes;
    }

    public static async Task<CommittedArtifactReference> ResolveAsync(
        CommittedFileSnapshotSource source,
        CommittedFileSnapshotOptions? options,
        CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(source);
        options ??= new CommittedFileSnapshotOptions();
        ValidateOptions(options);
        var commitBytes = await ReadBoundedFileAsync(
            source.CommitPath,
            options.MaximumCommitBytes,
            cancellationToken);
        var artifact = ResolveArtifact(source, commitBytes);
        if (artifact.ByteLength > options.MaximumArtifactBytes)
        {
            throw new InvalidDataException(
                $"The committed artifact exceeds the " +
                $"{options.MaximumArtifactBytes}-byte limit.");
        }

        return artifact;
    }

    public static CommittedGenerationSnapshot ResolveGenerationManifest(
        string manifestPath,
        IReadOnlyDictionary<string, string> requiredArtifacts,
        int maximumCommitBytes = 64 * 1024)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(manifestPath);
        ArgumentNullException.ThrowIfNull(requiredArtifacts);
        if (maximumCommitBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(maximumCommitBytes));
        }

        var fullManifestPath = Path.GetFullPath(manifestPath);
        var bytes = ReadBoundedFile(fullManifestPath, maximumCommitBytes);
        return ParseGenerationManifest(
            fullManifestPath,
            bytes,
            requiredArtifacts);
    }

    public static async Task<CommittedGenerationSnapshot>
        ResolveGenerationManifestAsync(
            string manifestPath,
            IReadOnlyDictionary<string, string> requiredArtifacts,
            CancellationToken cancellationToken,
            int maximumCommitBytes = 64 * 1024)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(manifestPath);
        ArgumentNullException.ThrowIfNull(requiredArtifacts);
        if (maximumCommitBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(maximumCommitBytes));
        }

        var fullManifestPath = Path.GetFullPath(manifestPath);
        var bytes = await ReadBoundedFileAsync(
            fullManifestPath,
            maximumCommitBytes,
            cancellationToken);
        return ParseGenerationManifest(
            fullManifestPath,
            bytes,
            requiredArtifacts);
    }

    internal static void VerifyArtifact(
        CommittedArtifactReference artifact,
        ReadOnlySpan<byte> bytes)
    {
        if (bytes.Length != artifact.ByteLength)
        {
            throw new InvalidDataException(
                $"Committed artifact '{artifact.ArtifactPath}' has length " +
                $"{bytes.Length}, expected {artifact.ByteLength}.");
        }

        Span<byte> actualDigest = stackalloc byte[SHA256.HashSizeInBytes];
        SHA256.HashData(bytes, actualDigest);
        var expectedDigest = Convert.FromHexString(artifact.Sha256);
        if (!CryptographicOperations.FixedTimeEquals(
                actualDigest,
                expectedDigest))
        {
            throw new InvalidDataException(
                $"Committed artifact '{artifact.ArtifactPath}' does not match " +
                "its sha256 digest.");
        }
    }

    private static CommittedArtifactReference ResolveArtifact(
        CommittedFileSnapshotSource source,
        ReadOnlySpan<byte> commitBytes)
    {
        if (!source.IsGenerationManifest)
        {
            return ParseArtifactDescriptor(source.CommitPath, commitBytes);
        }

        var generation = ParseGenerationManifest(
            source.CommitPath,
            commitBytes,
            source.RequiredArtifacts!);
        return generation.Artifacts[source.LogicalName!];
    }

    private static CommittedArtifactReference ParseArtifactDescriptor(
        string descriptorPath,
        ReadOnlySpan<byte> bytes)
    {
        var descriptor = DeserializeStrict<ArtifactDescriptor>(
            bytes,
            "The committed artifact descriptor is invalid.");
        if (!string.Equals(
                descriptor.Format,
                ArtifactDescriptorFormat,
                StringComparison.Ordinal) ||
            descriptor.Version != FormatVersion)
        {
            throw new InvalidDataException(
                "The committed artifact descriptor format or version is invalid.");
        }

        var root = Path.GetDirectoryName(descriptorPath)!;
        return ResolveArtifactReference(
            root,
            descriptor.Artifact,
            descriptor.ByteLength,
            descriptor.Sha256,
            expectedFileName: null);
    }

    private static CommittedGenerationSnapshot ParseGenerationManifest(
        string manifestPath,
        ReadOnlySpan<byte> bytes,
        IReadOnlyDictionary<string, string> requiredArtifacts)
    {
        var manifest = DeserializeStrict<GenerationManifest>(
            bytes,
            "The committed generation manifest is invalid.");
        if (!string.Equals(
                manifest.Format,
                GenerationManifestFormat,
                StringComparison.Ordinal) ||
            manifest.Version != FormatVersion ||
            string.IsNullOrWhiteSpace(manifest.Generation) ||
            !GenerationPattern().IsMatch(manifest.Generation) ||
            manifest.Files is null)
        {
            throw new InvalidDataException(
                "The committed generation manifest format is invalid.");
        }

        var root = Path.GetDirectoryName(manifestPath)!;
        var generationRoot = Path.GetFullPath(
            Path.Combine(root, "generations", manifest.Generation));
        EnsureDirectoryPath(root, generationRoot);

        var artifacts =
            new Dictionary<string, CommittedArtifactReference>(
                StringComparer.Ordinal);
        foreach (var (logicalName, expectedFileName) in requiredArtifacts)
        {
            if (!LogicalNamePattern().IsMatch(logicalName) ||
                !IsSafeArtifactFileName(expectedFileName) ||
                !manifest.Files.TryGetValue(logicalName, out var entry) ||
                entry is null)
            {
                throw new InvalidDataException(
                    $"The committed generation manifest is missing " +
                    $"'{logicalName}'.");
            }

            artifacts.Add(
                logicalName,
                ResolveArtifactReference(
                    generationRoot,
                    entry.Artifact,
                    entry.ByteLength,
                    entry.Sha256,
                    expectedFileName));
        }

        return new CommittedGenerationSnapshot(
            manifest.Generation,
            artifacts);
    }

    private static CommittedArtifactReference ResolveArtifactReference(
        string allowedRoot,
        string? artifactName,
        long? byteLength,
        string? sha256,
        string? expectedFileName)
    {
        if (string.IsNullOrWhiteSpace(artifactName) ||
            !IsSafeArtifactFileName(artifactName) ||
            expectedFileName is not null &&
            !string.Equals(
                artifactName,
                expectedFileName,
                StringComparison.Ordinal) ||
            byteLength is null or < 0 ||
            string.IsNullOrWhiteSpace(sha256) ||
            !Sha256Pattern().IsMatch(sha256))
        {
            throw new InvalidDataException(
                "A committed artifact identity, length, or sha256 digest is invalid.");
        }

        var root = Path.GetFullPath(allowedRoot);
        var artifactPath = Path.GetFullPath(Path.Combine(root, artifactName));
        if (!PathEquals(Path.GetDirectoryName(artifactPath)!, root))
        {
            throw new InvalidDataException(
                "A committed artifact path escapes its allowed directory.");
        }

        EnsureRegularFilePath(root, artifactPath);
        return new CommittedArtifactReference(
            artifactPath,
            byteLength.Value,
            sha256);
    }

    private static T DeserializeStrict<T>(
        ReadOnlySpan<byte> bytes,
        string message)
    {
        try
        {
            StrictJson.Validate(bytes);
            return JsonSerializer.Deserialize<T>(bytes, JsonOptions) ??
                throw new InvalidDataException(message);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(message, error);
        }
    }

    private static byte[] ReadBoundedFile(string path, int maximumBytes)
    {
        using var stream = OpenRead(path);
        return ReadBoundedFile(stream, path, maximumBytes);
    }

    private static async Task<byte[]> ReadBoundedFileAsync(
        string path,
        int maximumBytes,
        CancellationToken cancellationToken)
    {
        await using var stream = OpenRead(path);
        var length = stream.Length;
        if (length < 0 || length > maximumBytes)
        {
            throw new InvalidDataException(
                $"Commit file '{path}' exceeds the {maximumBytes}-byte limit.");
        }

        var bytes = GC.AllocateUninitializedArray<byte>((int)length);
        await stream.ReadExactlyAsync(bytes, cancellationToken);
        if (stream.ReadByte() != -1)
        {
            throw new IOException(
                $"Commit file '{path}' changed while it was read.");
        }

        return bytes;
    }

    private static byte[] ReadBoundedFile(
        FileStream stream,
        string path,
        int maximumBytes)
    {
        var length = stream.Length;
        if (length < 0 || length > maximumBytes)
        {
            throw new InvalidDataException(
                $"Commit file '{path}' exceeds the {maximumBytes}-byte limit.");
        }

        var bytes = GC.AllocateUninitializedArray<byte>((int)length);
        stream.ReadExactly(bytes);
        if (stream.ReadByte() != -1)
        {
            throw new IOException(
                $"Commit file '{path}' changed while it was read.");
        }

        return bytes;
    }

    private static async Task<byte[]> ReadExactArtifactAsync(
        CommittedArtifactReference artifact,
        CancellationToken cancellationToken)
    {
        await using var stream = OpenRead(artifact.ArtifactPath);
        if (stream.Length != artifact.ByteLength)
        {
            throw new InvalidDataException(
                $"Committed artifact '{artifact.ArtifactPath}' has length " +
                $"{stream.Length}, expected {artifact.ByteLength}.");
        }

        var bytes = GC.AllocateUninitializedArray<byte>(
            checked((int)artifact.ByteLength));
        await stream.ReadExactlyAsync(bytes, cancellationToken);
        if (stream.ReadByte() != -1)
        {
            throw new InvalidDataException(
                $"Committed artifact '{artifact.ArtifactPath}' changed length " +
                "while it was read.");
        }

        return bytes;
    }

    private static FileStream OpenRead(string path) =>
        new(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete,
            4096,
            FileOptions.Asynchronous | FileOptions.SequentialScan);

    private static void EnsureDirectoryPath(
        string root,
        string directoryPath)
    {
        var fullRoot = Path.GetFullPath(root);
        var fullDirectory = Path.GetFullPath(directoryPath);
        var relative = Path.GetRelativePath(fullRoot, fullDirectory);
        if (relative == ".." ||
            relative.StartsWith(
                $"..{Path.DirectorySeparatorChar}",
                StringComparison.Ordinal) ||
            Path.IsPathRooted(relative))
        {
            throw new InvalidDataException(
                "The committed generation path escapes its allowed root.");
        }

        RejectReparsePoint(fullRoot);
        var current = fullRoot;
        foreach (var component in relative.Split(
                     Path.DirectorySeparatorChar,
                     StringSplitOptions.RemoveEmptyEntries))
        {
            current = Path.Combine(current, component);
            RejectReparsePoint(current);
        }
    }

    private static void EnsureRegularFilePath(
        string root,
        string artifactPath)
    {
        EnsureDirectoryPath(root, Path.GetDirectoryName(artifactPath)!);
        RejectReparsePoint(artifactPath);
    }

    private static void RejectReparsePoint(string path)
    {
        try
        {
            if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0)
            {
                throw new InvalidDataException(
                    $"Committed path '{path}' cannot be a symbolic link or " +
                    "reparse point.");
            }
        }
        catch (FileNotFoundException)
        {
        }
        catch (DirectoryNotFoundException)
        {
        }
    }

    private static bool PathEquals(string left, string right) =>
        string.Equals(
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(left)),
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(right)),
            OperatingSystem.IsWindows()
                ? StringComparison.OrdinalIgnoreCase
                : StringComparison.Ordinal);

    private static void ValidateOptions(CommittedFileSnapshotOptions options)
    {
        if (options.MaximumArtifactBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The artifact byte limit must be positive.");
        }

        if (options.MaximumCommitBytes <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The commit byte limit must be positive.");
        }

        if (options.MaximumGenerationSwitchRetries < 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(options),
                "The generation switch retry count cannot be negative.");
        }
    }

    internal static bool IsSafeArtifactFileName(string? artifactName) =>
        artifactName is not null &&
        SafeArtifactNamePattern().IsMatch(artifactName);

    [GeneratedRegex(
        "\\A\\d+-\\d+-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex GenerationPattern();

    [GeneratedRegex(
        "\\A[a-z][a-z0-9-]*\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex LogicalNamePattern();

    [GeneratedRegex(
        "\\A[A-Za-z0-9][A-Za-z0-9._-]{0,127}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex SafeArtifactNamePattern();

    [GeneratedRegex(
        "\\A[0-9a-f]{64}\\z",
        RegexOptions.CultureInvariant)]
    private static partial Regex Sha256Pattern();

    private sealed record ArtifactDescriptor(
        string? Format,
        int? Version,
        string? Artifact,
        long? ByteLength,
        string? Sha256);

    private sealed record GenerationManifest(
        string? Format,
        int? Version,
        string? Generation,
        IReadOnlyDictionary<string, ArtifactDescriptorEntry?>? Files);

    private sealed record ArtifactDescriptorEntry(
        string? Artifact,
        long? ByteLength,
        string? Sha256);
}

public static class CommittedFileSnapshotWriter
{
    public static async Task<CommittedArtifactPublication> PublishAsync(
        string commitDescriptorPath,
        ReadOnlyMemory<byte> bytes,
        CancellationToken cancellationToken = default,
        string? artifactStem = null) =>
        await PublishCoreAsync(
            commitDescriptorPath,
            bytes,
            cancellationToken,
            artifactStem,
            observer: null);

    internal static async Task<CommittedArtifactPublication>
        PublishObservedAsync(
            string commitDescriptorPath,
            ReadOnlyMemory<byte> bytes,
            ICommittedFileSnapshotWriterObserver observer,
            CancellationToken cancellationToken = default,
            string? artifactStem = null,
            IDurableDirectoryOperations? directoryOperations = null) =>
        await PublishCoreAsync(
            commitDescriptorPath,
            bytes,
            cancellationToken,
            artifactStem,
            observer,
            directoryOperations);

    private static async Task<CommittedArtifactPublication> PublishCoreAsync(
        string commitDescriptorPath,
        ReadOnlyMemory<byte> bytes,
        CancellationToken cancellationToken,
        string? artifactStem,
        ICommittedFileSnapshotWriterObserver? observer,
        IDurableDirectoryOperations? directoryOperations = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(commitDescriptorPath);
        var descriptorPath = Path.GetFullPath(commitDescriptorPath);
        var directory = Path.GetDirectoryName(descriptorPath)!;

        artifactStem ??=
            Path.GetFileNameWithoutExtension(descriptorPath)
                .Replace(".commit", string.Empty, StringComparison.Ordinal);
        var artifactName = $"{artifactStem}-{Guid.NewGuid():N}.json";
        if (!CommittedFileSnapshot.IsSafeArtifactFileName(artifactName))
        {
            throw new ArgumentException(
                "The generated immutable artifact filename is unsafe.",
                nameof(artifactStem));
        }

        if (directoryOperations is null)
        {
            DurableDirectory.Create(directory);
        }
        else
        {
            DurableDirectory.Create(directory, directoryOperations);
        }
        var artifactPath = Path.Combine(directory, artifactName);
        var stagingDescriptorPath =
            $"{descriptorPath}.{Environment.ProcessId}.{Guid.NewGuid():N}.tmp";
        var descriptorPublished = false;

        try
        {
            await WriteDurableNewFileAsync(
                artifactPath,
                bytes,
                cancellationToken);
            await NotifyAsync(
                observer,
                CommittedFileSnapshotPublicationStage
                    .BeforeArtifactDirectorySync,
                cancellationToken);
            DurableDirectory.Flush(directory);
            await NotifyAsync(
                observer,
                CommittedFileSnapshotPublicationStage
                    .AfterArtifactDirectorySync,
                cancellationToken);
            var sha256 = Convert.ToHexString(SHA256.HashData(bytes.Span))
                .ToLowerInvariant();
            var descriptorBytes = JsonSerializer.SerializeToUtf8Bytes(
                new
                {
                    format = CommittedFileSnapshot.ArtifactDescriptorFormat,
                    version = 1,
                    artifact = artifactName,
                    byteLength = bytes.Length,
                    sha256
                });
            await WriteDurableNewFileAsync(
                stagingDescriptorPath,
                descriptorBytes,
                cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            File.Move(
                stagingDescriptorPath,
                descriptorPath,
                overwrite: true);
            descriptorPublished = true;
            await NotifyAsync(
                observer,
                CommittedFileSnapshotPublicationStage.AfterDescriptorRename,
                cancellationToken);
            DurableDirectory.Flush(directory);
            return new CommittedArtifactPublication(
                descriptorPath,
                artifactPath,
                bytes.Length,
                sha256);
        }
        catch
        {
            TryDelete(stagingDescriptorPath);
            if (!descriptorPublished)
            {
                TryDelete(artifactPath);
            }
            throw;
        }
    }

    private static ValueTask NotifyAsync(
        ICommittedFileSnapshotWriterObserver? observer,
        CommittedFileSnapshotPublicationStage stage,
        CancellationToken cancellationToken) =>
        observer?.OnStageAsync(stage, cancellationToken)
        ?? ValueTask.CompletedTask;

    private static async Task WriteDurableNewFileAsync(
        string path,
        ReadOnlyMemory<byte> bytes,
        CancellationToken cancellationToken)
    {
        await using var stream = new FileStream(
            path,
            FileMode.CreateNew,
            FileAccess.Write,
            FileShare.Read,
            4096,
            FileOptions.Asynchronous |
            FileOptions.SequentialScan |
            FileOptions.WriteThrough);
        await stream.WriteAsync(bytes, cancellationToken);
        await stream.FlushAsync(cancellationToken);
        stream.Flush(flushToDisk: true);
    }

    private static void TryDelete(string path)
    {
        try
        {
            File.Delete(path);
        }
        catch
        {
        }
    }
}

internal enum CommittedFileSnapshotPublicationStage
{
    BeforeArtifactDirectorySync,
    AfterArtifactDirectorySync,
    AfterDescriptorRename
}

internal interface ICommittedFileSnapshotWriterObserver
{
    ValueTask OnStageAsync(
        CommittedFileSnapshotPublicationStage stage,
        CancellationToken cancellationToken);
}

internal interface ICommittedFileSnapshotObserver
{
    ValueTask AfterCommitReadAsync(
        int attempt,
        CommittedArtifactReference artifact,
        CancellationToken cancellationToken);

    ValueTask AfterArtifactReadAsync(
        int attempt,
        CommittedArtifactReference artifact,
        CancellationToken cancellationToken);
}
