using Flaggo.Shared.Contracts;

namespace Flaggo.Decisioning.Tests;

public sealed class DurableDirectoryTests
{
    [Fact]
    public void Create_NestedMissingPath_SyncsEachParentBeforeItsChild()
    {
        var ancestor = Path.GetFullPath(ArtifactPath("durable-create"));
        var child = Path.Combine(ancestor, "first");
        var target = Path.Combine(child, "second");
        var operations = new RecordingDirectoryOperations(ancestor);

        DurableDirectory.Create(target, operations);

        Assert.Equal(
            [
                $"sync:{Path.GetDirectoryName(ancestor)!}",
                $"sync:{ancestor}",
                $"mkdir:{child}",
                $"sync:{ancestor}",
                $"sync:{child}",
                $"mkdir:{target}",
                $"sync:{child}",
                $"sync:{target}",
                $"sync:{child}",
                $"sync:{target}"
            ],
            operations.Events);
    }

    [Fact]
    public void Create_ExistingPath_SynchronizesRequestedBoundary()
    {
        var path = Path.GetFullPath(ArtifactPath("durable-existing"));
        var parent = Path.GetDirectoryName(path)!;
        var operations = new RecordingDirectoryOperations(path);

        DurableDirectory.Create(path, operations);

        Assert.Equal(
            [$"sync:{parent}", $"sync:{path}"],
            operations.Events);
    }

    [Fact]
    public void Create_VolumeRoot_SynchronizesRootOnce()
    {
        var root = Path.GetPathRoot(Path.GetFullPath(ArtifactPath("root")))!;
        var operations = new RecordingDirectoryOperations(root);

        DurableDirectory.Create(root, operations);

        Assert.Equal([$"sync:{root}"], operations.Events);
    }

    [Fact]
    public void Create_ConcurrentCreatorRace_Converges()
    {
        var ancestor = Path.GetFullPath(ArtifactPath("durable-race"));
        var target = Path.Combine(ancestor, "first", "second");
        var operations = new RecordingDirectoryOperations(
            ancestor,
            raceOnFirstCreate: true);

        DurableDirectory.Create(target, operations);

        Assert.True(operations.Exists(target));
        Assert.Equal(2, operations.Events.Count(
            item => item.StartsWith("mkdir:", StringComparison.Ordinal)));
    }

    [Fact]
    public async Task Create_ConcurrentFileSystemCreators_Converge()
    {
        var path = Path.Combine(
            ArtifactPath("durable-concurrent"),
            "first",
            "second");
        try
        {
            await Task.WhenAll(
                Enumerable.Range(0, 8)
                    .Select(_ => Task.Run(() => DurableDirectory.Create(path))));

            Assert.True(Directory.Exists(path));
        }
        finally
        {
            var root = Path.GetDirectoryName(Path.GetDirectoryName(path)!)!;
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }
        }
    }

    [Fact]
    public async Task Create_ConcurrentIntermediateAncestor_IsStabilizedBeforeDescendants()
    {
        for (var attempt = 0; attempt < 10; attempt++)
        {
            var ancestor = Path.GetFullPath(
                ArtifactPath($"durable-intermediate-hook-{attempt}"));
            var intermediate = Path.Combine(ancestor, "first");
            var target = Path.Combine(intermediate, "second");
            var state = new ConcurrentDirectoryState(ancestor);
            using var created = new ManualResetEventSlim();
            using var releaseCreator = new ManualResetEventSlim();
            var creatorAOperations = new HookedDirectoryOperations(
                state,
                afterCreate: _ =>
                {
                    created.Set();
                    if (!releaseCreator.Wait(TimeSpan.FromSeconds(10)))
                    {
                        throw new TimeoutException(
                            "Timed out waiting to release creator A.");
                    }
                });
            var creatorBOperations = new HookedDirectoryOperations(state);
            var creatorA = Task.Run(
                () => DurableDirectory.Create(
                    intermediate,
                    creatorAOperations));

            Assert.True(created.Wait(TimeSpan.FromSeconds(10)));
            try
            {
                DurableDirectory.Create(target, creatorBOperations);

                Assert.Equal(
                    [
                        $"sync:{ancestor}",
                        $"sync:{intermediate}",
                        $"mkdir:{target}",
                        $"sync:{intermediate}",
                        $"sync:{target}",
                        $"sync:{intermediate}",
                        $"sync:{target}"
                    ],
                    creatorBOperations.Events);
            }
            finally
            {
                releaseCreator.Set();
                await creatorA;
            }
        }
    }

    [Fact]
    public async Task Create_ConcurrentVisibleDirectory_SecondCreatorEstablishesBoundary()
    {
        for (var attempt = 0; attempt < 10; attempt++)
        {
            var ancestor = Path.GetFullPath(
                ArtifactPath($"durable-hook-{attempt}"));
            var target = Path.Combine(ancestor, "target");
            var state = new ConcurrentDirectoryState(ancestor);
            using var created = new ManualResetEventSlim();
            using var releaseCreator = new ManualResetEventSlim();
            var creatorAOperations = new HookedDirectoryOperations(
                state,
                afterCreate: _ =>
                {
                    created.Set();
                    if (!releaseCreator.Wait(TimeSpan.FromSeconds(10)))
                    {
                        throw new TimeoutException(
                            "Timed out waiting to release creator A.");
                    }
                });
            var creatorBOperations = new HookedDirectoryOperations(state);
            var creatorA = Task.Run(
                () => DurableDirectory.Create(target, creatorAOperations));

            Assert.True(created.Wait(TimeSpan.FromSeconds(10)));
            try
            {
                DurableDirectory.Create(target, creatorBOperations);

                Assert.Equal(
                    [$"sync:{ancestor}", $"sync:{target}"],
                    creatorBOperations.Events);
            }
            finally
            {
                releaseCreator.Set();
                await creatorA;
            }
        }
    }

    [Fact]
    public async Task Create_ConcurrentBoundaryFailure_IsPropagatedRepeatedly()
    {
        for (var attempt = 0; attempt < 10; attempt++)
        {
            var ancestor = Path.GetFullPath(
                ArtifactPath($"durable-hook-failure-{attempt}"));
            var target = Path.Combine(ancestor, "target");
            var state = new ConcurrentDirectoryState(ancestor);
            using var created = new ManualResetEventSlim();
            using var releaseCreator = new ManualResetEventSlim();
            var creatorAOperations = new HookedDirectoryOperations(
                state,
                afterCreate: _ =>
                {
                    created.Set();
                    if (!releaseCreator.Wait(TimeSpan.FromSeconds(10)))
                    {
                        throw new TimeoutException(
                            "Timed out waiting to release creator A.");
                    }
                });
            var expected = new IOException("Injected boundary sync failure.");
            var failurePath = attempt % 2 == 0 ? ancestor : target;
            var creatorBOperations = new HookedDirectoryOperations(
                state,
                flushFailure: path =>
                    path == failurePath ? expected : null);
            var creatorA = Task.Run(
                () => DurableDirectory.Create(target, creatorAOperations));

            Assert.True(created.Wait(TimeSpan.FromSeconds(10)));
            try
            {
                Assert.Same(
                    expected,
                    Assert.Throws<IOException>(
                        () => DurableDirectory.Create(
                            target,
                            creatorBOperations)));
                Assert.Equal(
                    failurePath == ancestor
                        ? [$"sync:{ancestor}"]
                        : [$"sync:{ancestor}", $"sync:{target}"],
                    creatorBOperations.Events);
            }
            finally
            {
                releaseCreator.Set();
                await creatorA;
            }
        }
    }

    [Fact]
    public void Flush_SynchronizesDirectoryOnCurrentPlatform()
    {
        var path = ArtifactPath("durable-directory");
        try
        {
            Directory.CreateDirectory(path);
            File.WriteAllText(Path.Combine(path, "record.json"), "{}");

            DurableDirectory.Flush(path);
        }
        finally
        {
            Directory.Delete(path, recursive: true);
        }
    }

    [Theory]
    [InlineData(22, true)]
    [InlineData(45, true)]
    [InlineData(95, true)]
    [InlineData(13, false)]
    public void UnixUnsupportedClassification_DoesNotHidePermissionFailures(
        int error,
        bool expected)
    {
        Assert.Equal(expected, DurableDirectory.IsUnsupportedUnixError(error));
    }

    [Theory]
    [InlineData(1, true)]
    [InlineData(50, true)]
    [InlineData(87, true)]
    [InlineData(5, false)]
    public void WindowsUnsupportedClassification_DoesNotHidePermissionFailures(
        int error,
        bool expected)
    {
        Assert.Equal(expected, DurableDirectory.IsUnsupportedWindowsError(error));
    }

    private static string ArtifactPath(string prefix) =>
        Path.Combine(
            Directory.GetCurrentDirectory(),
            ".flaggo",
            "test-artifacts",
            $"{prefix}-{Guid.NewGuid():N}");

    private sealed class RecordingDirectoryOperations(
        string existingAncestor,
        bool raceOnFirstCreate = false) : IDurableDirectoryOperations
    {
        private readonly HashSet<string> _directories =
            new(StringComparer.OrdinalIgnoreCase)
            {
                Path.GetFullPath(existingAncestor)
            };
        private bool _raceOnFirstCreate = raceOnFirstCreate;

        public List<string> Events { get; } = [];

        public bool Exists(string path) =>
            _directories.Contains(Path.GetFullPath(path));

        public void Create(string path)
        {
            path = Path.GetFullPath(path);
            Events.Add($"mkdir:{path}");
            _directories.Add(path);
            if (_raceOnFirstCreate)
            {
                _raceOnFirstCreate = false;
                throw new IOException("Concurrent creator won.");
            }
        }

        public void Flush(string path) =>
            Events.Add($"sync:{Path.GetFullPath(path)}");
    }

    private sealed class ConcurrentDirectoryState(string existingAncestor)
    {
        private readonly Lock _lock = new();
        private readonly HashSet<string> _directories =
            new(StringComparer.OrdinalIgnoreCase)
            {
                Path.GetFullPath(existingAncestor)
            };

        public bool Exists(string path)
        {
            lock (_lock)
            {
                return _directories.Contains(Path.GetFullPath(path));
            }
        }

        public void Create(string path)
        {
            lock (_lock)
            {
                _directories.Add(Path.GetFullPath(path));
            }
        }
    }

    private sealed class HookedDirectoryOperations(
        ConcurrentDirectoryState state,
        Action<string>? afterCreate = null,
        Func<string, Exception?>? flushFailure = null) :
        IDurableDirectoryOperations
    {
        public List<string> Events { get; } = [];

        public bool Exists(string path) => state.Exists(path);

        public void Create(string path)
        {
            path = Path.GetFullPath(path);
            Events.Add($"mkdir:{path}");
            state.Create(path);
            afterCreate?.Invoke(path);
        }

        public void Flush(string path)
        {
            path = Path.GetFullPath(path);
            Events.Add($"sync:{path}");
            var failure = flushFailure?.Invoke(path);
            if (failure is not null)
            {
                throw failure;
            }
        }
    }
}
