using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class CommittedFileSnapshotTests
{
    [Fact]
    public async Task MixedValidJsonPausedBeforeCommit_FailsDigestValidation()
    {
        using var file = new CommittedTestJsonFile("committed-mixed");
        var oldBytes = Encoding.UTF8.GetBytes(
            """{"version":1,"value":800,"tail":"complete"}""");
        var newBytes = Encoding.UTF8.GetBytes(
            """{"version":1,"value":900,"tail":"complete"}""");
        Assert.Equal(oldBytes.Length, newBytes.Length);
        await file.WriteAsync(oldBytes);

        var split = Array.IndexOf(newBytes, (byte)'9') + 3;
        await using var pausedWriter = new FileStream(
            file.ArtifactPath,
            FileMode.Open,
            FileAccess.Write,
            FileShare.ReadWrite | FileShare.Delete);
        await pausedWriter.WriteAsync(newBytes.AsMemory(0, split));
        pausedWriter.Flush(flushToDisk: true);

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => ReadAsync(file.Path));

        Assert.Contains("sha256 digest", error.Message);
        await using var mixedStream = new FileStream(
            file.ArtifactPath,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete);
        using var mixed = await JsonDocument.ParseAsync(mixedStream);
        Assert.Equal(900, mixed.RootElement.GetProperty("value").GetInt32());
        Assert.False(pausedWriter.SafeFileHandle.IsClosed);
    }

    [Fact]
    public async Task ArtifactChangedAfterDescriptorPublication_FailsClosed()
    {
        using var file = new CommittedTestJsonFile("committed-changed");
        var original = Encoding.UTF8.GetBytes("""{"version":1,"value":800}""");
        var changed = Encoding.UTF8.GetBytes("""{"version":1,"value":900}""");
        await file.WriteAsync(original);
        await File.WriteAllBytesAsync(file.ArtifactPath, changed);

        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => ReadAsync(file.Path));

        Assert.Contains("sha256 digest", error.Message);
    }

    [Fact]
    public async Task DescriptorSwitchDuringRead_RetriesToCompleteNewSnapshot()
    {
        using var file = new CommittedTestJsonFile("committed-switch");
        var oldBytes = Encoding.UTF8.GetBytes("""{"generation":"old"}""");
        var newBytes = Encoding.UTF8.GetBytes("""{"generation":"new"}""");
        await file.WriteAsync(oldBytes);
        var observer = new DescriptorSwitchObserver(
            async () => await file.WriteAsync(newBytes));

        var bytes = await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            new CommittedFileSnapshotOptions
            {
                MaximumGenerationSwitchRetries = 2,
                Observer = observer
            },
            CancellationToken.None);

        Assert.True(bytes.AsSpan().SequenceEqual(newBytes));
        Assert.Equal(2, observer.CommitReads);
        Assert.Equal(2, observer.ArtifactReads);
    }

    [Fact]
    public async Task DescriptorSwitchEveryRead_StopsAtBoundedRetryLimit()
    {
        using var file = new CommittedTestJsonFile("committed-switch-limit");
        await file.WriteAsync("""{"generation":"initial"}""");
        var observer = new RepeatedSwitchObserver(
            attempt => file.WriteAsync(
                $$"""{"generation":"attempt-{{attempt}}"}"""));

        var error = await Assert.ThrowsAsync<IOException>(
            () => CommittedFileSnapshot.ReadAsync(
                CommittedFileSnapshotSource.FromDescriptor(file.Path),
                new CommittedFileSnapshotOptions
                {
                    MaximumGenerationSwitchRetries = 2,
                    Observer = observer
                },
                CancellationToken.None));

        Assert.Contains("changed during every bounded read", error.Message);
        Assert.Equal(3, observer.ArtifactReads);
    }

    [Theory]
    [InlineData("format")]
    [InlineData("version")]
    [InlineData("digest")]
    [InlineData("digest-whitespace")]
    [InlineData("length")]
    [InlineData("missing")]
    public async Task CorruptedDescriptor_IsRejected(string corruption)
    {
        using var file = new CommittedTestJsonFile($"committed-{corruption}");
        var bytes = Encoding.UTF8.GetBytes("""{"version":1}""");
        await file.WriteAsync(bytes);
        var artifact = System.IO.Path.GetFileName(file.ArtifactPath);
        var sha256 = Convert.ToHexString(
                System.Security.Cryptography.SHA256.HashData(bytes))
            .ToLowerInvariant();
        await file.WriteDescriptorAsync(
            JsonSerializer.Serialize(
                new
                {
                    format = corruption == "format"
                        ? "flaggo.unknown"
                        : CommittedFileSnapshot.ArtifactDescriptorFormat,
                    version = corruption == "version" ? 2 : 1,
                    artifact = corruption == "missing"
                        ? "missing.json"
                        : artifact,
                    byteLength = corruption == "length"
                        ? bytes.Length + 1
                        : bytes.Length,
                    sha256 = corruption switch
                    {
                        "digest" => new string('f', 64),
                        "digest-whitespace" => $"{sha256} ",
                        _ => sha256
                    }
                }));

        var error = await Record.ExceptionAsync(() => ReadAsync(file.Path));
        Assert.True(
            error is InvalidDataException or FileNotFoundException,
            $"Unexpected exception: {error}");
    }

    [Theory]
    [InlineData("../outside.json")]
    [InlineData("..\\outside.json")]
    [InlineData("sibling/state.json")]
    [InlineData("sibling\\state.json")]
    public async Task DescriptorPathCannotEscapeSiblingDirectory(
        string artifact)
    {
        using var file = new CommittedTestJsonFile("committed-path");
        await file.WriteAsync("{}");
        await file.WriteDescriptorAsync(
            JsonSerializer.Serialize(
                new
                {
                    format = CommittedFileSnapshot.ArtifactDescriptorFormat,
                    version = 1,
                    artifact,
                    byteLength = 2,
                    sha256 = new string('0', 64)
                }));

        await Assert.ThrowsAsync<InvalidDataException>(
            () => ReadAsync(file.Path));
    }

    [Fact]
    public async Task StableRead_HasNoRetryTimerDelay()
    {
        using var file = new CommittedTestJsonFile("committed-performance");
        await file.WriteAsync("""{"version":1}""");
        var observer = new CountingObserver();
        var stopwatch = Stopwatch.StartNew();

        await CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(file.Path),
            new CommittedFileSnapshotOptions
            {
                MaximumGenerationSwitchRetries = 100,
                Observer = observer
            },
            CancellationToken.None);
        stopwatch.Stop();

        Assert.Equal(1, observer.CommitReads);
        Assert.Equal(1, observer.ArtifactReads);
        Assert.True(
            stopwatch.Elapsed < TimeSpan.FromSeconds(1),
            $"Stable committed read took {stopwatch.Elapsed}.");
    }

    public static TheoryData<string> InvalidArtifactStems()
    {
        var cases = new TheoryData<string>
        {
            "-state",
            "_state",
            "state/escape",
            "state\\escape",
            "st\u00e1te",
            new('a', 91),
            new('a', 129)
        };
        return cases;
    }

    [Theory]
    [MemberData(nameof(InvalidArtifactStems))]
    public async Task Writer_RejectsUnsafeFinalArtifactNameBeforePublication(
        string artifactStem)
    {
        var directory = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"committed-invalid-name-{Guid.NewGuid():N}");
        Directory.CreateDirectory(directory);
        var descriptorPath = Path.Combine(directory, "current.commit.json");
        try
        {
            await Assert.ThrowsAsync<ArgumentException>(
                () => CommittedFileSnapshotWriter.PublishAsync(
                    descriptorPath,
                    Encoding.UTF8.GetBytes("{}"),
                    artifactStem: artifactStem));

            Assert.Empty(Directory.EnumerateFileSystemEntries(directory));
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    [Fact]
    public async Task Writer_AcceptsMaximumLengthFinalArtifactName()
    {
        var directory = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"committed-boundary-name-{Guid.NewGuid():N}");
        var descriptorPath = Path.Combine(directory, "current.commit.json");
        try
        {
            var publication = await CommittedFileSnapshotWriter.PublishAsync(
                descriptorPath,
                Encoding.UTF8.GetBytes("{}"),
                artifactStem: new string('a', 90));

            Assert.Equal(
                128,
                Path.GetFileName(publication.ArtifactPath).Length);
            Assert.Equal("{}", Encoding.UTF8.GetString(
                await ReadAsync(descriptorPath)));
        }
        finally
        {
            if (Directory.Exists(directory))
            {
                Directory.Delete(directory, recursive: true);
            }
        }
    }

    [Fact]
    public async Task Writer_DirectoryParentSyncFailure_DoesNotPublishDescriptor()
    {
        var directory = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"committed-create-failure-{Guid.NewGuid():N}");
        var descriptorPath = Path.Combine(directory, "state.commit.json");
        var operations = new FailingDirectoryCreationOperations();
        try
        {
            var error = await Assert.ThrowsAsync<IOException>(
                () => CommittedFileSnapshotWriter.PublishObservedAsync(
                    descriptorPath,
                    Encoding.UTF8.GetBytes("""{"generation":"new"}"""),
                    new FailingPublicationObserver(
                        (CommittedFileSnapshotPublicationStage)(-1)),
                    CancellationToken.None,
                    directoryOperations: operations));

            Assert.Contains("parent sync", error.Message);
            Assert.False(File.Exists(descriptorPath));
            Assert.True(Directory.Exists(directory));
            Assert.Empty(Directory.EnumerateFileSystemEntries(directory));
        }
        finally
        {
            if (Directory.Exists(directory))
            {
                Directory.Delete(directory, recursive: true);
            }
        }
    }

    [Theory]
    [InlineData("BeforeArtifactDirectorySync", "old")]
    [InlineData("AfterArtifactDirectorySync", "old")]
    [InlineData("AfterDescriptorRename", "new")]
    public async Task Writer_FailureAtDurabilityBarrier_LeavesUsableDescriptor(
        string failureStageName,
        string expectedGeneration)
    {
        var failureStage = Enum.Parse<CommittedFileSnapshotPublicationStage>(
            failureStageName);
        var directory = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"committed-ordering-{Guid.NewGuid():N}");
        var descriptorPath = Path.Combine(directory, "state.commit.json");
        try
        {
            await CommittedFileSnapshotWriter.PublishAsync(
                descriptorPath,
                Encoding.UTF8.GetBytes("""{"generation":"old"}"""));
            var observer = new FailingPublicationObserver(failureStage);

            var error = await Assert.ThrowsAsync<IOException>(
                () => CommittedFileSnapshotWriter.PublishObservedAsync(
                    descriptorPath,
                    Encoding.UTF8.GetBytes("""{"generation":"new"}"""),
                    observer,
                    CancellationToken.None));

            Assert.Contains(failureStage.ToString(), error.Message);
            var resolved = JsonDocument.Parse(await ReadAsync(descriptorPath));
            Assert.Equal(
                expectedGeneration,
                resolved.RootElement.GetProperty("generation").GetString());
            if (
                failureStage
                == CommittedFileSnapshotPublicationStage.AfterDescriptorRename)
            {
                Assert.Equal(
                    [
                        CommittedFileSnapshotPublicationStage
                            .BeforeArtifactDirectorySync,
                        CommittedFileSnapshotPublicationStage
                            .AfterArtifactDirectorySync,
                        CommittedFileSnapshotPublicationStage
                            .AfterDescriptorRename
                    ],
                    observer.Stages);
            }
        }
        finally
        {
            if (Directory.Exists(directory))
            {
                Directory.Delete(directory, recursive: true);
            }
        }
    }

    private static Task<byte[]> ReadAsync(string descriptorPath) =>
        CommittedFileSnapshot.ReadAsync(
            CommittedFileSnapshotSource.FromDescriptor(descriptorPath),
            options: null,
            CancellationToken.None);

    private sealed class DescriptorSwitchObserver(Func<Task> switchDescriptor) :
        ICommittedFileSnapshotObserver
    {
        private bool _switched;

        public int CommitReads { get; private set; }

        public int ArtifactReads { get; private set; }

        public ValueTask AfterCommitReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken)
        {
            CommitReads++;
            return ValueTask.CompletedTask;
        }

        public async ValueTask AfterArtifactReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken)
        {
            ArtifactReads++;
            if (!_switched)
            {
                _switched = true;
                await switchDescriptor();
            }
        }
    }

    private sealed class CountingObserver : ICommittedFileSnapshotObserver
    {
        public int CommitReads { get; private set; }

        public int ArtifactReads { get; private set; }

        public ValueTask AfterCommitReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken)
        {
            CommitReads++;
            return ValueTask.CompletedTask;
        }

        public ValueTask AfterArtifactReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken)
        {
            ArtifactReads++;
            return ValueTask.CompletedTask;
        }
    }

    private sealed class RepeatedSwitchObserver(
        Func<int, Task> switchDescriptor) : ICommittedFileSnapshotObserver
    {
        public int ArtifactReads { get; private set; }

        public ValueTask AfterCommitReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken) =>
            ValueTask.CompletedTask;

        public async ValueTask AfterArtifactReadAsync(
            int attempt,
            CommittedArtifactReference artifact,
            CancellationToken cancellationToken)
        {
            ArtifactReads++;
            await switchDescriptor(attempt);
        }
    }

    private sealed class FailingPublicationObserver(
        CommittedFileSnapshotPublicationStage failureStage) :
        ICommittedFileSnapshotWriterObserver
    {
        public List<CommittedFileSnapshotPublicationStage> Stages { get; } = [];

        public ValueTask OnStageAsync(
            CommittedFileSnapshotPublicationStage stage,
            CancellationToken cancellationToken)
        {
            Stages.Add(stage);
            if (stage == failureStage)
            {
                throw new IOException($"Injected failure at {stage}.");
            }

            return ValueTask.CompletedTask;
        }
    }

    private sealed class FailingDirectoryCreationOperations :
        IDurableDirectoryOperations
    {
        private bool _created;
        private bool _failed;

        public bool Exists(string path) => Directory.Exists(path);

        public void Create(string path)
        {
            Directory.CreateDirectory(path);
            _created = true;
        }

        public void Flush(string path)
        {
            if (_created && !_failed)
            {
                _failed = true;
                throw new IOException("Injected parent sync failure.");
            }

            DurableDirectory.Flush(path);
        }
    }
}
