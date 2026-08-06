using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class ExposureConfirmationServiceTests
{
    [Fact]
    public async Task ConfirmedReplay_ReturnsStoredResultWithoutAdditionalAudit()
    {
        var store = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-1");
        var audit = new ControllableExposureAuditSink();
        var service = new ExposureConfirmationService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        var first = await service.ConfirmAsync(
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);
        Assert.Single(audit.Records);
        Assert.Equal(1, audit.Attempts);

        audit.FailWrites = true;
        var replay = await service.ConfirmAsync(
            "decision-1",
            request,
            AppIds,
            Environments,
            CancellationToken.None);

        Assert.Equal(first, replay);
        Assert.Single(audit.Records);
        Assert.Equal(1, audit.Attempts);
        await Assert.ThrowsAsync<ExposureConfirmationConflictException>(
            () => service.ConfirmAsync(
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
        var service = new ExposureConfirmationService(store, audit);
        var request = new ExposureConfirmationRequest(
            "confirm-1",
            "2026-08-06T00:00:01Z");
        await store.CreatePendingAsync(
            "decision-1",
            request.ConfirmToken,
            Snapshot(),
            CancellationToken.None);

        await Assert.ThrowsAsync<IOException>(
            () => service.ConfirmAsync(
                "decision-1",
                request,
                AppIds,
                Environments,
                CancellationToken.None));
        Assert.Empty(audit.Records);
        Assert.Null(store.Find("decision-1")!.Confirmation);
        Assert.NotNull(store.Find("decision-1")!.PreparedConfirmation);

        var retried = await service.ConfirmAsync(
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

    private static IReadOnlySet<string> AppIds { get; } =
        new HashSet<string>(StringComparer.Ordinal) { "tetris-demo" };

    private static IReadOnlySet<string> Environments { get; } =
        new HashSet<string>(StringComparer.Ordinal) { "dev" };

    private static DecisionSnapshot Snapshot() => new(
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
        [],
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
            return Task.CompletedTask;
        }
    }
}
