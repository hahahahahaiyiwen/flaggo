using System.Diagnostics;
using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Shared.Contracts;

namespace Flaggo.State;

public sealed record LocalFileGovernedStateLifecycleStoreOptions(
    string CommitDescriptorPath,
    TimeSpan? LockTimeout = null,
    TimeSpan? LockRetryDelay = null);

internal interface IGovernedStateDocumentPublisher
{
    Task PublishAsync(
        string commitDescriptorPath,
        ReadOnlyMemory<byte> bytes,
        CancellationToken cancellationToken);
}

public sealed class LocalFileGovernedStateLifecycleStore :
    IGovernedStateLifecycleStore, ILifecycleAuditReader
{
    private readonly string _commitDescriptorPath;
    private readonly string _lockPath;
    private readonly TimeSpan _lockTimeout;
    private readonly TimeSpan _lockRetryDelay;
    private readonly TimeProvider _timeProvider;
    private readonly IGovernedStateIdentityGenerator _identityGenerator;
    private readonly IGovernedStateDocumentPublisher _publisher;
    private readonly Action<string> _ensureDirectory;
    private readonly Action<string> _flushDirectory;

    public LocalFileGovernedStateLifecycleStore(
        LocalFileGovernedStateLifecycleStoreOptions options,
        TimeProvider? timeProvider = null,
        IGovernedStateIdentityGenerator? identityGenerator = null)
        : this(
            options,
            timeProvider ?? TimeProvider.System,
            identityGenerator ?? new GuidGovernedStateIdentityGenerator(),
            new CommittedGovernedStateDocumentPublisher())
    {
    }

    internal LocalFileGovernedStateLifecycleStore(
        LocalFileGovernedStateLifecycleStoreOptions options,
        TimeProvider timeProvider,
        IGovernedStateIdentityGenerator identityGenerator,
        IGovernedStateDocumentPublisher publisher,
        IDurableDirectoryOperations? directoryOperations = null)
    {
        ArgumentNullException.ThrowIfNull(options);
        ArgumentException.ThrowIfNullOrWhiteSpace(options.CommitDescriptorPath);
        _commitDescriptorPath = Path.GetFullPath(options.CommitDescriptorPath);
        _lockPath = $"{_commitDescriptorPath}.lock";
        _lockTimeout = options.LockTimeout ?? TimeSpan.FromSeconds(10);
        _lockRetryDelay = options.LockRetryDelay ?? TimeSpan.FromMilliseconds(25);
        if (_lockTimeout <= TimeSpan.Zero || _lockRetryDelay <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(options), "Lifecycle lock waits must be positive.");
        }
        _timeProvider = timeProvider;
        _identityGenerator = identityGenerator;
        _publisher = publisher;
        _ensureDirectory = directoryOperations is null
            ? DurableDirectory.Create
            : path => DurableDirectory.Create(path, directoryOperations);
        _flushDirectory = directoryOperations is null ? DurableDirectory.Flush : directoryOperations.Flush;
    }

    public async Task<GovernedDecisionState?> GetBaselineAsync(
        GovernedStateAddress address,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).GetBaselineAsync(address, cancellationToken);

    public async Task<GovernedDecisionState?> GetStateAsync(
        string stateId,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).GetStateAsync(stateId, cancellationToken);

    public async Task<LifecycleReviewRecord?> GetReviewAsync(
        string reviewId,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).GetReviewAsync(reviewId, cancellationToken);

    public async Task<LifecycleActivationReceipt?> GetActivationAsync(
        string activationId,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).GetActivationAsync(activationId, cancellationToken);

    public async Task<LifecycleTransitionReceipt?> GetTransitionAsync(
        string transitionId,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).GetTransitionAsync(transitionId, cancellationToken);

    public Task<LifecycleReviewReceipt> ReplayReviewAsync(
        LifecycleReviewRequest request,
        LifecycleActor actor,
        CancellationToken cancellationToken) =>
        ReplayAsync(store => store.ReplayReviewAsync(request, actor, cancellationToken), cancellationToken);

    public Task<LifecycleActivationReceipt> ReplayActivationAsync(
        LifecycleActivationRequest request,
        LifecycleActor actor,
        CancellationToken cancellationToken) =>
        ReplayAsync(store => store.ReplayActivationAsync(request, actor, cancellationToken), cancellationToken);

    public async Task<LifecycleAuditTrail> ReadAsync(
        string appId,
        string environment,
        CancellationToken cancellationToken) =>
        await (await LoadAsync(cancellationToken)).ReadAsync(appId, environment, cancellationToken);

    public Task<LifecycleReviewReceipt> CommitReviewAsync(
        LifecycleReviewCommit commit,
        CancellationToken cancellationToken) =>
        MutateAsync(store => store.CommitReviewAsync(commit, cancellationToken), cancellationToken);

    public Task<LifecycleActivationReceipt> CommitActivationAsync(
        LifecycleActivationCommit commit,
        CancellationToken cancellationToken) =>
        MutateAsync(store => store.CommitActivationAsync(commit, cancellationToken), cancellationToken);

    public Task<LifecycleTransitionReceipt> CommitTransitionAsync(
        LifecycleTransitionCommit commit,
        CancellationToken cancellationToken) =>
        MutateAsync(store => store.CommitTransitionAsync(commit, cancellationToken), cancellationToken);

    private async Task<T> MutateAsync<T>(
        Func<InMemoryGovernedStateLifecycleStore, Task<T>> mutation,
        CancellationToken cancellationToken)
    {
        await using var lease = await AcquireLeaseAsync(cancellationToken);
        var store = await LoadAsync(cancellationToken);
        var previous = GovernedStatePersistence.Serialize(store.CapturePersistenceSnapshot());
        var result = await mutation(store);
        var replacement = GovernedStatePersistence.Serialize(store.CapturePersistenceSnapshot());
        if (replacement.Length > CommittedFileSnapshotOptions.DefaultMaximumArtifactBytes)
        {
            throw new InvalidDataException(
                $"The lifecycle journal exceeds the {CommittedFileSnapshotOptions.DefaultMaximumArtifactBytes}-byte reader limit.");
        }
        if (!previous.AsSpan().SequenceEqual(replacement))
        {
            await _publisher.PublishAsync(_commitDescriptorPath, replacement, cancellationToken);
        }
        else
        {
            ConfirmDurability(cancellationToken);
        }
        return result;
    }

    private async Task<T> ReplayAsync<T>(
        Func<InMemoryGovernedStateLifecycleStore, Task<T>> replay,
        CancellationToken cancellationToken)
    {
        await using var lease = await AcquireLeaseAsync(cancellationToken);
        var store = await LoadAsync(cancellationToken);
        var receipt = await replay(store);
        ConfirmDurability(cancellationToken);
        return receipt;
    }

    private void ConfirmDurability(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        // A previous writer may have renamed the descriptor but failed its final barrier.
        _flushDirectory(Path.GetDirectoryName(_commitDescriptorPath)!);
        cancellationToken.ThrowIfCancellationRequested();
    }

    private async Task<InMemoryGovernedStateLifecycleStore> LoadAsync(
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        GovernedStatePersistenceSnapshot snapshot;
        if (!DescriptorExists())
        {
            snapshot = new GovernedStatePersistenceSnapshot(3, [], [], [], new([], [], [], []));
        }
        else
        {
            var bytes = await CommittedFileSnapshot.ReadAsync(
                CommittedFileSnapshotSource.FromDescriptor(_commitDescriptorPath),
                options: null,
                cancellationToken);
            snapshot = GovernedStatePersistence.Deserialize(bytes);
        }
        return new InMemoryGovernedStateLifecycleStore(snapshot, _timeProvider, _identityGenerator);
    }

    private bool DescriptorExists()
    {
        try
        {
            if ((File.GetAttributes(_commitDescriptorPath) & FileAttributes.Directory) != 0)
            {
                throw new IOException("The lifecycle commit descriptor cannot be a directory.");
            }
            return true;
        }
        catch (FileNotFoundException)
        {
            return false;
        }
        catch (DirectoryNotFoundException)
        {
            return false;
        }
    }

    private async Task<FileStream> AcquireLeaseAsync(CancellationToken cancellationToken)
    {
        var directory = Path.GetDirectoryName(_commitDescriptorPath)
            ?? throw new InvalidOperationException("The governed-state path must include a directory.");
        cancellationToken.ThrowIfCancellationRequested();
        _ensureDirectory(directory);
        var started = Stopwatch.GetTimestamp();
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                return new FileStream(
                    _lockPath, FileMode.OpenOrCreate, FileAccess.ReadWrite,
                    FileShare.None, 1, FileOptions.Asynchronous);
            }
            catch (IOException) when (Stopwatch.GetElapsedTime(started) < _lockTimeout)
            {
                await Task.Delay(_lockRetryDelay, cancellationToken);
            }
            catch (IOException error)
            {
                throw new TimeoutException(
                    $"Timed out acquiring the governed-state lock '{_lockPath}'.", error);
            }
        }
    }

    private sealed class CommittedGovernedStateDocumentPublisher : IGovernedStateDocumentPublisher
    {
        public async Task PublishAsync(
            string commitDescriptorPath,
            ReadOnlyMemory<byte> bytes,
            CancellationToken cancellationToken)
        {
            await CommittedFileSnapshotWriter.PublishAsync(
                commitDescriptorPath, bytes, cancellationToken, artifactStem: "governed-state");
        }
    }
}

internal static class GovernedStatePersistence
{
    public static byte[] Serialize(GovernedStatePersistenceSnapshot snapshot)
    {
        ValidateShape(snapshot);
        return LifecycleJson.Bytes(snapshot);
    }

    public static GovernedStatePersistenceSnapshot Deserialize(ReadOnlyMemory<byte> bytes)
    {
        try
        {
            var snapshot = LifecycleJson.Read<GovernedStatePersistenceSnapshot>(bytes.Span);
            ValidateShape(snapshot);
            return snapshot;
        }
        catch (JsonException error)
        {
            throw new InvalidDataException(
                "The lifecycle governed-state document is not valid strict version 3 JSON.", error);
        }
    }

    private static void ValidateShape(GovernedStatePersistenceSnapshot snapshot)
    {
        if (snapshot.Version != 3 ||
            snapshot.States is null || snapshot.Activations is null ||
            snapshot.Transitions is null || snapshot.Journal is null ||
            snapshot.States.Any(entry => entry is null || entry.Address is null || entry.State is null) ||
            snapshot.Activations.Any(entry => entry is null) ||
            snapshot.Transitions.Any(entry => entry is null))
        {
            throw new InvalidDataException("A lifecycle governed-state document must use complete version 3 data.");
        }
    }
}
