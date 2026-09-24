using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace Flaggo.Decisioning.Tests;

public sealed class ExposureConfirmationServiceTests
{
    private static readonly TimeSpan ConcurrentTestTimeout =
        TimeSpan.FromSeconds(30);

    [Fact]
    public async Task ConfirmedReplay_ReturnsStoredResultWithoutAdditionalAudit()
    {
        var store = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var audit = new ControllableExposureAuditSink();
        var service = CreateService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var first = await service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        Assert.Single(audit.Records);
        Assert.Equal(1, audit.Attempts);

        audit.FailWrites = true;
        var replay = await service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);

        Assert.Equal(first, replay);
        Assert.Single(audit.Records);
        Assert.Equal(1, audit.Attempts);
        await Assert.ThrowsAsync<ExposureConfirmationConflictException>(
            () => service.ConfirmAsync("local-development",
                "decision-1",
                request with { AppliedAt = "2026-08-06T00:00:02Z" },
                AppIds,
                Environments,
                CancellationToken.None));
        Assert.Equal(1, audit.Attempts);
    }

    [Fact]
    public async Task PreparedRetry_AuditsOnceAndCommitsAfterPriorAuditFailure()
    {
        var store = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var audit = new ControllableExposureAuditSink
        {
            FailuresRemaining = 1
        };
        var service = CreateService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        await Assert.ThrowsAsync<IOException>(
            () => service.ConfirmAsync("local-development",
                "decision-1",
                request,
                AppIds,
                Environments,
                CancellationToken.None));
        Assert.Empty(audit.Records);
        Assert.Null(store.Find("decision-1")!.Confirmation);
        Assert.NotNull(store.Find("decision-1")!.PreparedConfirmation);

        var retried = await service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);

        Assert.Single(audit.Records);
        Assert.Equal(2, audit.Attempts);
        Assert.Equal(retried.Result, store.Find("decision-1")!.Confirmation);
        Assert.Null(store.Find("decision-1")!.PreparedConfirmation);
    }

    [Fact]
    public async Task ConflictingAuditIdentity_PropagatesAndDoesNotCommit()
    {
        var store = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var audit = new InMemoryAuditSink();
        await audit.RecordExposureAsync(
            new ExposureAuditRecord(
                "exposure-1",
                "other-decision",
                "tetris-demo",
                "dev",
                "2026-08-06T00:00:01Z",
                "2026-08-06T00:00:02Z"),
            CancellationToken.None);
        var service = CreateService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        await Assert.ThrowsAsync<ExposureAuditConflictException>(
            () => service.ConfirmAsync("local-development",
                "decision-1",
                request,
                AppIds,
                Environments,
                CancellationToken.None));

        Assert.Null(store.Find("decision-1")!.Confirmation);
        Assert.NotNull(store.Find("decision-1")!.PreparedConfirmation);
    }

    [Fact]
    public async Task RequestCanceledAfterAudit_StillCommitsPreparedConfirmation()
    {
        using var requestCancellation = new CancellationTokenSource();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new ControllableExposureStore(innerStore);
        var audit = new ControllableExposureAuditSink
        {
            AfterRecord = requestCancellation.Cancel
        };
        var service = CreateService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var outcome = await service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            requestCancellation.Token);

        Assert.True(requestCancellation.IsCancellationRequested);
        Assert.Equal(1, store.CommitAttempts);
        Assert.Equal(outcome.Result, innerStore.Find("decision-1")!.Confirmation);
    }

    [Fact]
    public async Task RequestCanceledDuringAudit_DoesNotCommit()
    {
        using var requestCancellation = new CancellationTokenSource();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new ControllableExposureStore(innerStore);
        var audit = new BlockingExposureAuditSink();
        var service = CreateService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var confirmation = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            requestCancellation.Token);
        await audit.Started;
        requestCancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            async () => await confirmation);
        Assert.Equal(0, store.CommitAttempts);
        Assert.NotNull(innerStore.Find("decision-1")!.PreparedConfirmation);
    }

    [Fact]
    public async Task NonCooperativeNeverCompletingCommit_TimesOutWithinBoundedTime()
    {
        var timeProvider = new ManualTimeProvider();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new NonCooperativeExposureStore(innerStore);
        var attempt = store.BlockNextCommit();
        var audit = new DeduplicatingExposureAuditSink();
        var service = new ExposureConfirmationService(
            store,
            audit,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                timeProvider,
                CancellationToken.None,
                NullLogger<BoundedPostAuditExposureCommitPolicy>.Instance));
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var confirmation = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        await attempt.Started.WaitAsync(TimeSpan.FromSeconds(5));
        timeProvider.Advance(TimeSpan.FromSeconds(5));

        await Assert.ThrowsAsync<TimeoutException>(
            async () => await confirmation.WaitAsync(TimeSpan.FromSeconds(5)));
        Assert.Null(innerStore.Find("decision-1")!.Confirmation);
        Assert.NotNull(innerStore.Find("decision-1")!.PreparedConfirmation);
        Assert.Equal(0, timeProvider.ActiveTimerCount);

        attempt.Succeed();
        await attempt.Execution.WaitAsync(TimeSpan.FromSeconds(5));
    }

    [Fact]
    public async Task NonCooperativeLateSuccess_RetryReturnsAcceptedWithoutDuplicateAudit()
    {
        var timeProvider = new ManualTimeProvider();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new NonCooperativeExposureStore(innerStore);
        var attempt = store.BlockNextCommit();
        var audit = new DeduplicatingExposureAuditSink();
        var service = new ExposureConfirmationService(
            store,
            audit,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                timeProvider,
                CancellationToken.None,
                NullLogger<BoundedPostAuditExposureCommitPolicy>.Instance));
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var confirmation = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        await attempt.Started.WaitAsync(TimeSpan.FromSeconds(5));
        timeProvider.Advance(TimeSpan.FromSeconds(5));
        await Assert.ThrowsAsync<TimeoutException>(
            async () => await confirmation.WaitAsync(TimeSpan.FromSeconds(5)));
        var prepared = innerStore.Find("decision-1")!.PreparedConfirmation;
        Assert.NotNull(prepared);

        var retryAttempt = store.BlockNextCommit();
        var retry = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        await retryAttempt.Started.WaitAsync(TimeSpan.FromSeconds(5));

        attempt.Succeed();
        await attempt.Execution.WaitAsync(TimeSpan.FromSeconds(5));
        retryAttempt.Succeed();
        await retryAttempt.Execution.WaitAsync(TimeSpan.FromSeconds(5));
        var retried = await retry.WaitAsync(TimeSpan.FromSeconds(5));

        Assert.Equal(prepared, retried.Result);
        Assert.Equal(retried.Result, innerStore.Find("decision-1")!.Confirmation);
        Assert.Equal(2, audit.Attempts);
        Assert.Single(audit.Records);
        Assert.Equal(2, store.CommitAttempts);
        Assert.Equal(0, timeProvider.ActiveTimerCount);
    }

    [Fact]
    public async Task NonCooperativeLateFailure_IsObservedAndRetrySucceeds()
    {
        var timeProvider = new ManualTimeProvider();
        var logger = new RecordingCommitPolicyLogger();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new NonCooperativeExposureStore(innerStore);
        var attempt = store.BlockNextCommit();
        var audit = new DeduplicatingExposureAuditSink();
        var service = new ExposureConfirmationService(
            store,
            audit,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                timeProvider,
                CancellationToken.None,
                logger));
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var confirmation = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        await attempt.Started.WaitAsync(TimeSpan.FromSeconds(5));
        timeProvider.Advance(TimeSpan.FromSeconds(5));
        await Assert.ThrowsAsync<TimeoutException>(
            async () => await confirmation.WaitAsync(TimeSpan.FromSeconds(5)));

        attempt.Fail(new IOException("late non-cooperative commit failure"));
        await attempt.Finished.WaitAsync(TimeSpan.FromSeconds(5));
        var observed = await logger.ErrorLogged.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(
            "late non-cooperative commit failure",
            observed.GetBaseException().Message);

        var retried = await service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);

        Assert.Equal(retried.Result, innerStore.Find("decision-1")!.Confirmation);
        Assert.Equal(2, audit.Attempts);
        Assert.Single(audit.Records);
        Assert.Equal(2, store.CommitAttempts);
        Assert.Equal(0, timeProvider.ActiveTimerCount);
    }

    [Fact]
    public async Task ApplicationShutdown_BoundsNonCooperativePostAuditCommit()
    {
        using var shutdown = new CancellationTokenSource();
        var timeProvider = new ManualTimeProvider();
        var innerStore = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var store = new NonCooperativeExposureStore(innerStore);
        var attempt = store.BlockNextCommit();
        var audit = new DeduplicatingExposureAuditSink();
        var service = new ExposureConfirmationService(
            store,
            audit,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                timeProvider,
                shutdown.Token,
                NullLogger<BoundedPostAuditExposureCommitPolicy>.Instance));
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var confirmation = service.ConfirmAsync("local-development",
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        try
        {
            await attempt.Started.WaitAsync(ConcurrentTestTimeout);
            shutdown.Cancel();

            await Assert.ThrowsAnyAsync<OperationCanceledException>(
                async () => await confirmation.WaitAsync(ConcurrentTestTimeout));
            Assert.Null(innerStore.Find("decision-1")!.Confirmation);
            Assert.NotNull(innerStore.Find("decision-1")!.PreparedConfirmation);
            Assert.Equal(0, timeProvider.ActiveTimerCount);
        }
        finally
        {
            shutdown.Cancel();
            attempt.Succeed();
        }

        await attempt.Execution.WaitAsync(ConcurrentTestTimeout);
    }

    private static ExposureConfirmationService CreateService(
        IExposureStore store,
        IExposureAuditSink audit) =>
        new(
            store,
            audit,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                TimeProvider.System,
                CancellationToken.None,
                NullLogger<BoundedPostAuditExposureCommitPolicy>.Instance));

    private static IReadOnlySet<string> AppIds { get; } =
        new HashSet<string>(StringComparer.Ordinal) { "tetris-demo" };

    private static IReadOnlySet<string> Environments { get; } =
        new HashSet<string>(StringComparer.Ordinal) { "dev" };

    private static DecisionSnapshot Snapshot() => new(
        "local-development",
        "tetris-demo",
        "dev",
        new RuntimeContractIdentity(
            "def-phase3",
            $"sha256:{new string('a', 64)}",
            "rev-phase3"),
        JsonSerializer.SerializeToElement(850),
        "number",
        new ServerFallbackInfo("server", false, false, null),
        new Dictionary<string, JsonElement>(),
        new Dictionary<string, JsonElement>(),
        new DecisionTargetRef("session", "game-1"),
        new DecisionTargetRef("cohort", "new_players"),
        [],
        ["session:game-1", "cohort:new_players", "global"],
        new PolicyEvaluationResult("approved", [], []));

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 8, 6, 0, 0, 2, TimeSpan.Zero);
    }

    private sealed class ControllableExposureAuditSink : IExposureAuditSink
    {
        public int Attempts { get; private set; }

        public int FailuresRemaining { get; set; }

        public bool FailWrites { get; set; }

        public Action? AfterRecord { get; set; }

        public List<ExposureAuditRecord> Records { get; } = [];

        public Task RecordExposureAsync(
            ExposureAuditRecord record,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            Attempts++;
            if (FailWrites)
            {
                throw new IOException("audit unavailable");
            }

            if (FailuresRemaining > 0)
            {
                FailuresRemaining--;
                throw new IOException("audit unavailable");
            }

            Records.Add(record);
            AfterRecord?.Invoke();
            return Task.CompletedTask;
        }
    }

    private sealed class BlockingExposureAuditSink : IExposureAuditSink
    {
        private readonly TaskCompletionSource _started =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public Task Started => _started.Task;

        public async Task RecordExposureAsync(
            ExposureAuditRecord record,
            CancellationToken cancellationToken)
        {
            _ = record;
            _started.TrySetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, cancellationToken);
        }
    }

    private sealed class DeduplicatingExposureAuditSink : IExposureAuditSink
    {
        private readonly Dictionary<string, ExposureAuditRecord> _records =
            new(StringComparer.Ordinal);

        public int Attempts { get; private set; }

        public IReadOnlyCollection<ExposureAuditRecord> Records => _records.Values;

        public Task RecordExposureAsync(
            ExposureAuditRecord record,
            CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            Attempts++;
            if (_records.TryGetValue(record.ExposureId, out var existing) &&
                existing != record)
            {
                throw new InvalidDataException(
                    "The exposure ID was reused for a different audit record.");
            }

            _records[record.ExposureId] = record;
            return Task.CompletedTask;
        }
    }

    private sealed class ControllableExposureStore(IExposureStore inner)
        : IExposureStore
    {
        private readonly TaskCompletionSource _commitStarted =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public bool HangCommits { get; set; }

        public int CommitAttempts { get; private set; }

        public Task CommitStarted => _commitStarted.Task;

        public Task CreatePendingAsync(
            string decisionId,
            string confirmToken,
            DecisionSnapshot snapshot,
            CancellationToken cancellationToken) =>
            inner.CreatePendingAsync(
                decisionId,
                confirmToken,
                snapshot,
                cancellationToken);

        public Task RemovePendingAsync(
            string decisionId,
            CancellationToken cancellationToken) =>
            inner.RemovePendingAsync(decisionId, cancellationToken);

        public Task<ExposureConfirmationOutcome?> FindReplayAsync(
            string tenantId,
            string decisionId,
            ExposureConfirmationRequest request,
            IReadOnlySet<string> appIds,
            IReadOnlySet<string> environments,
            CancellationToken cancellationToken) =>
            inner.FindReplayAsync(tenantId,
                decisionId,
                request,
                appIds,
                environments,
                cancellationToken);

        public Task<ExposureConfirmationPreparation> PrepareConfirmationAsync(
            string tenantId,
            string decisionId,
            ExposureConfirmationRequest request,
            IReadOnlySet<string> appIds,
            IReadOnlySet<string> environments,
            CancellationToken cancellationToken) =>
            inner.PrepareConfirmationAsync(tenantId,
                decisionId,
                request,
                appIds,
                environments,
                cancellationToken);

        public async Task CommitConfirmationAsync(
            string decisionId,
            string exposureId,
            CancellationToken cancellationToken)
        {
            CommitAttempts++;
            _commitStarted.TrySetResult();
            if (HangCommits)
            {
                await Task.Delay(Timeout.InfiniteTimeSpan, cancellationToken);
            }

            await inner.CommitConfirmationAsync(
                decisionId,
                exposureId,
                cancellationToken);
        }
    }

    private sealed class NonCooperativeExposureStore(IExposureStore inner)
        : IExposureStore
    {
        private readonly Queue<CommitAttempt> _blockedCommits = [];

        public int CommitAttempts { get; private set; }

        public CommitAttempt BlockNextCommit()
        {
            var attempt = new CommitAttempt();
            _blockedCommits.Enqueue(attempt);
            return attempt;
        }

        public Task CreatePendingAsync(
            string decisionId,
            string confirmToken,
            DecisionSnapshot snapshot,
            CancellationToken cancellationToken) =>
            inner.CreatePendingAsync(
                decisionId,
                confirmToken,
                snapshot,
                cancellationToken);

        public Task RemovePendingAsync(
            string decisionId,
            CancellationToken cancellationToken) =>
            inner.RemovePendingAsync(decisionId, cancellationToken);

        public Task<ExposureConfirmationOutcome?> FindReplayAsync(
            string tenantId,
            string decisionId,
            ExposureConfirmationRequest request,
            IReadOnlySet<string> appIds,
            IReadOnlySet<string> environments,
            CancellationToken cancellationToken) =>
            inner.FindReplayAsync(tenantId,
                decisionId,
                request,
                appIds,
                environments,
                cancellationToken);

        public Task<ExposureConfirmationPreparation> PrepareConfirmationAsync(
            string tenantId,
            string decisionId,
            ExposureConfirmationRequest request,
            IReadOnlySet<string> appIds,
            IReadOnlySet<string> environments,
            CancellationToken cancellationToken) =>
            inner.PrepareConfirmationAsync(tenantId,
                decisionId,
                request,
                appIds,
                environments,
                cancellationToken);

        public Task CommitConfirmationAsync(
            string decisionId,
            string exposureId,
            CancellationToken cancellationToken)
        {
            _ = cancellationToken;
            CommitAttempts++;
            if (!_blockedCommits.TryDequeue(out var attempt))
            {
                return inner.CommitConfirmationAsync(
                    decisionId,
                    exposureId,
                    CancellationToken.None);
            }

            var execution = CompleteCommitAsync(attempt, decisionId, exposureId);
            attempt.SetExecution(execution);
            return execution;
        }

        private async Task CompleteCommitAsync(
            CommitAttempt attempt,
            string decisionId,
            string exposureId)
        {
            attempt.MarkStarted();
            try
            {
                var failure = await attempt.WaitForReleaseAsync();
                if (failure is not null)
                {
                    throw failure;
                }

                await inner.CommitConfirmationAsync(
                    decisionId,
                    exposureId,
                    CancellationToken.None);
            }
            finally
            {
                attempt.MarkFinished();
            }
        }
    }

    private sealed class CommitAttempt
    {
        private readonly TaskCompletionSource _started =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource<Exception?> _release =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource<Task> _execution =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource _finished =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public Task Started => _started.Task;

        public Task Execution => _execution.Task.Unwrap();

        public Task Finished => _finished.Task;

        public void Succeed() => _release.TrySetResult(null);

        public void Fail(Exception exception) => _release.TrySetResult(exception);

        public void MarkStarted() => _started.TrySetResult();

        public Task<Exception?> WaitForReleaseAsync() => _release.Task;

        public void SetExecution(Task execution) =>
            _execution.TrySetResult(execution);

        public void MarkFinished() => _finished.TrySetResult();
    }

    private sealed class RecordingCommitPolicyLogger
        : ILogger<BoundedPostAuditExposureCommitPolicy>
    {
        private readonly TaskCompletionSource<Exception> _errorLogged =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public Task<Exception> ErrorLogged => _errorLogged.Task;

        public IDisposable? BeginScope<TState>(TState state)
            where TState : notnull =>
            null;

        public bool IsEnabled(LogLevel logLevel) => logLevel >= LogLevel.Error;

        public void Log<TState>(
            LogLevel logLevel,
            EventId eventId,
            TState state,
            Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            _ = eventId;
            _ = state;
            _ = formatter;
            if (logLevel >= LogLevel.Error && exception is not null)
            {
                _errorLogged.TrySetResult(exception);
            }
        }
    }

    private sealed class ManualTimeProvider : TimeProvider
    {
        private readonly List<ManualTimer> _timers = [];

        public int ActiveTimerCount => _timers.Count(timer => !timer.IsDisposed);

        public override ITimer CreateTimer(
            TimerCallback callback,
            object? state,
            TimeSpan dueTime,
            TimeSpan period)
        {
            var timer = new ManualTimer(callback, state, dueTime, period);
            _timers.Add(timer);
            return timer;
        }

        public void Advance(TimeSpan elapsed)
        {
            foreach (var timer in _timers.ToArray())
            {
                timer.Advance(elapsed);
            }
        }

        private sealed class ManualTimer(
            TimerCallback callback,
            object? state,
            TimeSpan dueTime,
            TimeSpan period) : ITimer
        {
            private TimeSpan _dueTime = dueTime;
            private TimeSpan _period = period;
            private bool _disposed;

            public bool IsDisposed => _disposed;

            public bool Change(TimeSpan dueTime, TimeSpan period)
            {
                if (_disposed)
                {
                    return false;
                }

                _dueTime = dueTime;
                _period = period;
                return true;
            }

            public void Dispose()
            {
                _disposed = true;
            }

            public ValueTask DisposeAsync()
            {
                Dispose();
                return ValueTask.CompletedTask;
            }

            public void Advance(TimeSpan elapsed)
            {
                if (_disposed || _dueTime == Timeout.InfiniteTimeSpan)
                {
                    return;
                }

                if (elapsed < _dueTime)
                {
                    _dueTime -= elapsed;
                    return;
                }

                _dueTime = _period == Timeout.InfiniteTimeSpan
                    ? Timeout.InfiniteTimeSpan
                    : _period;
                callback(state);
            }
        }
    }
}
