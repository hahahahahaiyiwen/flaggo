using System.Text;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

internal sealed class CommittedTestJsonFile : IDisposable
{
    private readonly string _directory;

    public CommittedTestJsonFile(string prefix)
    {
        _directory = System.IO.Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"{prefix}-{Guid.NewGuid():N}");
        Path = System.IO.Path.Combine(_directory, "current.commit.json");
    }

    public string Path { get; }

    public string ArtifactPath { get; private set; } = string.Empty;

    public Task WriteAsync(string content) =>
        WriteAsync(Encoding.UTF8.GetBytes(content));

    public async Task WriteAsync(ReadOnlyMemory<byte> bytes)
    {
        var publication = await CommittedFileSnapshotWriter.PublishAsync(
            Path,
            bytes,
            artifactStem: "snapshot");
        ArtifactPath = publication.ArtifactPath;
    }

    public Task WriteDescriptorAsync(string content) =>
        File.WriteAllTextAsync(Path, content);

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }
}
