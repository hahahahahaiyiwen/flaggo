using System.Text.Json;
using Flaggo.Evidence;
using Flaggo.Lifecycle;
using Flaggo.Policy;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace Flaggo.Decisioning.Tests;

public sealed class LifecycleReplayDurabilityTests
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task VisibleReceiptCannotAcknowledgeAnOutstandingOrFailedPublication(bool activation)
    {
        using var file = TestJsonFile.CreateCommitted("replay-post-rename");
        var stable = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock());
        if (activation)
        {
            await Invoke(CreateService(stable), false);
        }
        var publisher = new PausedAfterRenamePublisher();
        var logger = new LateFailureLogger();
        var writer = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock(),
            new GuidGovernedStateIdentityGenerator(), publisher);
        var pending = Invoke(CreateService(writer, new(TimeSpan.FromSeconds(1)), logger: logger), activation);
        object original;
        try
        {
            await publisher.Renamed.Task.WaitAsync(TimeSpan.FromSeconds(5));
            await Assert.ThrowsAsync<TimeoutException>(() => pending.WaitAsync(TimeSpan.FromSeconds(5)));
            original = activation
                ? (await stable.GetActivationAsync("activation-1", CancellationToken.None))!
                : (await stable.GetReviewAsync("review-1", CancellationToken.None))!.Receipt;
            Assert.True(LeaseHeld(file.Path));
            await Assert.ThrowsAsync<TimeoutException>(() =>
                Invoke(CreateService(stable, new(TimeSpan.FromSeconds(1))), activation)
                    .WaitAsync(TimeSpan.FromSeconds(5)));
            Assert.True(LeaseHeld(file.Path));
        }
        finally
        {
            publisher.Release.TrySetException(new IOException("Injected failure before the final directory barrier."));
            await logger.LateFailure.Task.WaitAsync(TimeSpan.FromSeconds(5));
        }

        var descriptor = await File.ReadAllBytesAsync(file.Path);
        var journal = LifecycleJson.Digest(await stable.ReadAsync("app", "dev", CancellationToken.None));
        using var barriers = new ReplayDirectoryOperations(file.Path) { Fail = true };
        var recovered = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock(),
            new GuidGovernedStateIdentityGenerator(), new UnexpectedPublisher(), barriers);
        var policy = new TestPolicy { Fail = true };
        var retry = CreateService(recovered, policy: policy);
        await Assert.ThrowsAsync<IOException>(() => Invoke(retry, activation));
        Assert.Equal(1, barriers.UnderLeaseFlushes);
        Assert.Equal(descriptor, await File.ReadAllBytesAsync(file.Path));
        Assert.False(LeaseHeld(file.Path));

        barriers.Fail = false;
        var replay = await Invoke(retry, activation);
        Assert.Equal(2, barriers.UnderLeaseFlushes);
        Assert.Equal(LifecycleJson.Digest(original), LifecycleJson.Digest(replay));
        Assert.Equal(descriptor, await File.ReadAllBytesAsync(file.Path));
        Assert.Equal(journal, LifecycleJson.Digest(await stable.ReadAsync("app", "dev", CancellationToken.None)));
    }

    [Theory]
    [InlineData("review")]
    [InlineData("activation")]
    [InlineData("transition")]
    public async Task UnchangedCommitRetriesAlsoRequireTheUnderLeaseDurabilityBarrier(string operation)
    {
        using var file = TestJsonFile.CreateCommitted("replay-commit-barrier");
        var stable = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock());
        await LifecycleTestData.ReviewAsync(stable);
        var review = (await stable.GetReviewAsync("review-1", CancellationToken.None))!;
        Func<IGovernedStateLifecycleStore, Task<object>> replay;
        if (operation == "review")
        {
            replay = async store => await store.CommitReviewAsync(review.Commit, CancellationToken.None);
        }
        else if (operation == "activation")
        {
            var commit = new LifecycleActivationCommit(new("activation-1", "review-1"), LifecycleTestData.Actor,
                LifecycleIdentity.InputsDigest(review.Commit), review.Commit.Decision);
            replay = async store => await store.CommitActivationAsync(commit, CancellationToken.None);
        }
        else
        {
            var activated = await LifecycleTestData.ActivateAsync(stable);
            var commit = new LifecycleTransitionCommit(
                new("transition-1", new("app", "dev", "decision", LifecycleTestData.Target),
                    activated.StateId!, Assert.IsType<long>(activated.Generation),
                    GovernedDecisionStateStatus.Completed, "Complete the proposal."),
                LifecycleTestData.Actor);
            replay = async store => await store.CommitTransitionAsync(commit, CancellationToken.None);
        }
        var original = await replay(stable);
        var descriptor = await File.ReadAllBytesAsync(file.Path);
        using var barriers = new ReplayDirectoryOperations(file.Path) { Fail = true };
        var retried = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock(),
            new GuidGovernedStateIdentityGenerator(), new UnexpectedPublisher(), barriers);

        await Assert.ThrowsAsync<IOException>(() => replay(retried));
        Assert.Equal(1, barriers.UnderLeaseFlushes);
        barriers.Fail = false;
        Assert.Equal(LifecycleJson.Digest(original), LifecycleJson.Digest(await replay(retried)));
        Assert.Equal(2, barriers.UnderLeaseFlushes);
        Assert.Equal(descriptor, await File.ReadAllBytesAsync(file.Path));
    }

    [Theory]
    [InlineData("timeout")]
    [InlineData("caller")]
    [InlineData("shutdown")]
    public async Task BlockedReplayBarrierKeepsItsLeaseAfterBoundedWaitingEnds(string interruption)
    {
        using var file = TestJsonFile.CreateCommitted("replay-bounded-barrier");
        var stable = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock());
        var original = await Invoke(CreateService(stable), false);
        using var barriers = new ReplayDirectoryOperations(file.Path) { Block = true };
        var blocked = new LocalFileGovernedStateLifecycleStore(new(file.Path), new Clock(),
            new GuidGovernedStateIdentityGenerator(), new UnexpectedPublisher(), barriers);
        using var caller = new CancellationTokenSource();
        using var shutdown = new CancellationTokenSource();
        var service = CreateService(blocked, new(
            interruption == "timeout" ? TimeSpan.FromSeconds(1) : TimeSpan.FromSeconds(10), shutdown.Token));
        var pending = Invoke(service, false, caller.Token);
        try
        {
            await barriers.Entered.Task.WaitAsync(TimeSpan.FromSeconds(5));
            if (interruption == "caller")
            {
                caller.Cancel();
            }
            if (interruption == "shutdown")
            {
                shutdown.Cancel();
            }
            if (interruption == "timeout")
            {
                await Assert.ThrowsAsync<TimeoutException>(() => pending.WaitAsync(TimeSpan.FromSeconds(5)));
            }
            else
            {
                await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending.WaitAsync(TimeSpan.FromSeconds(5)));
            }
            Assert.True(LeaseHeld(file.Path));
        }
        finally
        {
            barriers.Release.Set();
            using var completion = new CancellationTokenSource(TimeSpan.FromSeconds(5));
            var confirmed = await stable.ReplayReviewAsync(
                new("review-1", LifecycleTestData.Proposal()), LifecycleTestData.Actor, completion.Token);
            Assert.Equal(LifecycleJson.Digest(original), LifecycleJson.Digest(confirmed));
        }
    }

    [Theory]
    [InlineData(false, "missing")]
    [InlineData(true, "missing")]
    [InlineData(false, "actor")]
    [InlineData(true, "actor")]
    [InlineData(false, "content")]
    [InlineData(true, "content")]
    public async Task ReplayPortsCannotCreateOperationsOrBypassActorAndContentChecks(bool activation, string scenario)
    {
        var store = new InMemoryGovernedStateLifecycleStore(new Clock());
        await LifecycleTestData.ReviewAsync(store);
        await LifecycleTestData.ActivateAsync(store);
        var before = LifecycleJson.Digest(await store.ReadAsync("app", "dev", CancellationToken.None));
        var actor = scenario == "actor"
            ? LifecycleTestData.Actor with { CanReview = false, CanActivate = false }
            : LifecycleTestData.Actor;
        Task Replay() => activation
            ? store.ReplayActivationAsync(new(scenario == "missing" ? "missing" : "activation-1",
                scenario == "content" ? "other-review" : "review-1"), actor, CancellationToken.None)
            : store.ReplayReviewAsync(new(scenario == "missing" ? "missing" : "review-1",
                LifecycleTestData.Proposal() with
                {
                    Value = JsonSerializer.SerializeToElement(scenario == "content" ? 850 : 800)
                }), actor, CancellationToken.None);

        if (scenario == "content")
        {
            await Assert.ThrowsAsync<GovernedStateConflictException>(Replay);
        }
        else
        {
            var error = await Assert.ThrowsAsync<GovernedStateValidationException>(Replay);
            Assert.Equal(scenario == "actor" ? "actor-not-authorized"
                : activation ? "activation-not-found" : "review-not-found", error.Code);
        }
        Assert.Equal(before, LifecycleJson.Digest(await store.ReadAsync("app", "dev", CancellationToken.None)));
    }

    private static ProposalGovernance CreateService(
        IGovernedStateLifecycleStore store,
        LifecycleCommitOptions? options = null,
        TestPolicy? policy = null,
        ILogger<ProposalGovernance>? logger = null)
    {
        var registry = new InMemoryDefinitionRegistry([LifecycleTestData.Runtime()],
            intelligenceDefinitions: [LifecycleTestData.Intelligence()]);
        return new(registry, registry, policy ?? new TestPolicy(), new InMemoryProposalEvidenceReader([]),
            new Actors(), new DefaultLifecyclePolicyEvaluator(), store, new Clock(),
            logger ?? NullLogger<ProposalGovernance>.Instance, options);
    }

    private static async Task<object> Invoke(
        ProposalGovernance service, bool activation, CancellationToken cancellationToken = default) =>
        activation
            ? await service.ActivateAsync(new("activation-1", "review-1"), cancellationToken)
            : await service.ReviewAsync(new("review-1", LifecycleTestData.Proposal()), cancellationToken);

    private static bool LeaseHeld(string descriptorPath)
    {
        try
        {
            using var lease = new FileStream($"{descriptorPath}.lock", FileMode.Open, FileAccess.ReadWrite, FileShare.None);
            return false;
        }
        catch (IOException)
        {
            return true;
        }
    }

    private sealed class Clock : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() => LifecycleTestData.Now;
    }

    private sealed class Actors : ILifecycleActorProvider
    {
        public Task<LifecycleActor> GetAsync(string appId, string environment, CancellationToken cancellationToken) =>
            Task.FromResult(LifecycleTestData.Actor);
    }

    private sealed class TestPolicy : ILifecyclePolicyContextProvider
    {
        public bool Fail { get; init; }
        public Task<LifecyclePolicyContext?> GetAsync(
            GovernedDefinitionIdentity definition, CancellationToken cancellationToken) =>
            Fail ? throw new IOException("Replay must not resolve changing policy.")
                : Task.FromResult<LifecyclePolicyContext?>(LifecycleTestData.Policy);
    }

    private sealed class UnexpectedPublisher : IGovernedStateDocumentPublisher
    {
        public Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken) =>
            throw new InvalidOperationException("Replay must not publish another artifact.");
    }

    private sealed class PausedAfterRenamePublisher :
        IGovernedStateDocumentPublisher, ICommittedFileSnapshotWriterObserver
    {
        public TaskCompletionSource Renamed { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        public async Task PublishAsync(string path, ReadOnlyMemory<byte> bytes, CancellationToken cancellationToken) =>
            await CommittedFileSnapshotWriter.PublishObservedAsync(path, bytes, this, CancellationToken.None);

        public ValueTask OnStageAsync(CommittedFileSnapshotPublicationStage stage, CancellationToken cancellationToken)
        {
            if (stage != CommittedFileSnapshotPublicationStage.AfterDescriptorRename)
            {
                return ValueTask.CompletedTask;
            }
            Renamed.TrySetResult();
            return new ValueTask(Release.Task);
        }
    }

    private sealed class ReplayDirectoryOperations(string descriptorPath) : IDurableDirectoryOperations, IDisposable
    {
        public bool Fail { get; set; }
        public bool Block { get; init; }
        public int UnderLeaseFlushes { get; private set; }
        public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public ManualResetEventSlim Release { get; } = new();
        public bool Exists(string path) => Directory.Exists(path);
        public void Create(string path) => Directory.CreateDirectory(path);
        public void Flush(string path)
        {
            if (path == Path.GetDirectoryName(descriptorPath) && LeaseHeld(descriptorPath))
            {
                UnderLeaseFlushes++;
                Entered.TrySetResult();
                if (Block && !Release.Wait(TimeSpan.FromSeconds(15)))
                {
                    throw new TimeoutException("The test did not release the replay barrier.");
                }
                if (Fail)
                {
                    throw new IOException("Injected replay durability barrier failure.");
                }
            }
            DurableDirectory.Flush(path);
        }
        public void Dispose() => Release.Dispose();
    }

    private sealed class LateFailureLogger : ILogger<ProposalGovernance>
    {
        public TaskCompletionSource<Exception> LateFailure { get; } =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;
        public bool IsEnabled(LogLevel logLevel) => true;
        public void Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            if (exception is not null && formatter(state, exception).Contains("after its caller stopped waiting"))
            {
                LateFailure.TrySetResult(exception);
            }
        }
    }
}
