using System.Text;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

internal class TestJsonFile : IDisposable
{
    private readonly string _artifactsDirectory;
    private readonly string? _committedFileDirectory;
    private readonly bool _usesCommittedFile;

    public TestJsonFile(string prefix)
        : this(prefix, usesCommittedFile: false)
    {
    }

    protected TestJsonFile(string prefix, bool usesCommittedFile)
    {
        _usesCommittedFile = usesCommittedFile;
        _artifactsDirectory = System.IO.Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts");

        var uniqueName = $"{prefix}-{Guid.NewGuid():N}";
        if (usesCommittedFile)
        {
            _committedFileDirectory = System.IO.Path.Combine(
                _artifactsDirectory,
                uniqueName);
            Path = System.IO.Path.Combine(
                _committedFileDirectory,
                "current.commit.json");
        }
        else
        {
            Directory.CreateDirectory(_artifactsDirectory);
            Path = System.IO.Path.Combine(
                _artifactsDirectory,
                $"{uniqueName}.json");
        }
    }

    public string Path { get; }

    public string ArtifactPath { get; private set; } = string.Empty;

    public static TestJsonFile CreateCommitted(string prefix) =>
        new(prefix, usesCommittedFile: true);

    public Task WriteAsync(string content) =>
        WriteAsync(Encoding.UTF8.GetBytes(content));

    public async Task WriteAsync(ReadOnlyMemory<byte> bytes)
    {
        if (_usesCommittedFile)
        {
            var publication = await CommittedFileSnapshotWriter.PublishAsync(
                Path,
                bytes,
                artifactStem: "snapshot");
            ArtifactPath = publication.ArtifactPath;
            return;
        }

        await File.WriteAllBytesAsync(Path, bytes.ToArray());
    }

    public Task WriteDescriptorAsync(string content) =>
        File.WriteAllTextAsync(Path, content);

    public void Dispose()
    {
        if (_committedFileDirectory is not null)
        {
            if (Directory.Exists(_committedFileDirectory))
            {
                Directory.Delete(_committedFileDirectory, recursive: true);
            }
            return;
        }

        if (File.Exists(Path))
        {
            File.Delete(Path);
        }

        var lockPath = $"{Path}.lock";
        if (File.Exists(lockPath))
        {
            File.Delete(lockPath);
        }

        var dataDirectory = $"{Path}.d";
        if (Directory.Exists(dataDirectory))
        {
            Directory.Delete(dataDirectory, recursive: true);
        }

        var name = System.IO.Path.GetFileName(Path);
        if (Directory.Exists(_artifactsDirectory))
        {
            foreach (var stagingDirectory in Directory.GetDirectories(
                         _artifactsDirectory,
                         $"{name}.d.tmp-*"))
            {
                Directory.Delete(stagingDirectory, recursive: true);
            }
        }
    }
}

internal sealed class CommittedTestJsonFile(string prefix) :
    TestJsonFile(prefix, usesCommittedFile: true)
{
}
