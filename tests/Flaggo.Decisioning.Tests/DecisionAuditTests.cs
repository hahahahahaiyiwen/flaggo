using Flaggo.Audit;

namespace Flaggo.Decisioning.Tests;

public sealed class DecisionAuditTests
{
    public static TheoryData<string, ExposureAuditRecord> ExposureConflicts()
    {
        var record = ExposureRecord();
        return new TheoryData<string, ExposureAuditRecord>
        {
            { "decisionId", record with { DecisionId = "decision-2" } },
            { "appId", record with { AppId = "other-app" } },
            { "environment", record with { Environment = "prod" } },
            { "appliedAt", record with { AppliedAt = null } },
            {
                "confirmedAt",
                record with { ConfirmedAt = "2026-08-06T00:00:03Z" }
            }
        };
    }

    [Fact]
    public async Task InMemoryExposureAudit_ExactReplayDoesNotAppend()
    {
        var sink = new InMemoryAuditSink();
        var record = ExposureRecord();

        await sink.RecordExposureAsync(record, CancellationToken.None);
        await sink.RecordExposureAsync(record, CancellationToken.None);

        Assert.Single(sink.ExposureRecords);
    }

    [Theory]
    [MemberData(nameof(ExposureConflicts))]
    public async Task InMemoryExposureAudit_ConflictingIdentityFailsClosed(
        string field,
        ExposureAuditRecord conflict)
    {
        _ = field;
        var sink = new InMemoryAuditSink();
        await sink.RecordExposureAsync(
            ExposureRecord(),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<ExposureAuditConflictException>(
            () => sink.RecordExposureAsync(conflict, CancellationToken.None));

        Assert.Equal("exposure-1", error.ExposureId);
        Assert.Single(sink.ExposureRecords);
    }

    [Fact]
    public async Task InMemoryExposureAudit_ConcurrentConflictHasOneWinner()
    {
        var sink = new InMemoryAuditSink();
        var first = ExposureRecord();
        var second = first with { DecisionId = "decision-2" };
        var start = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var attempts = new[] { first, second }
            .Select(record => Task.Run(async () =>
            {
                await start.Task;
                return await Record.ExceptionAsync(
                    () => sink.RecordExposureAsync(
                        record,
                        CancellationToken.None));
            }))
            .ToArray();

        start.SetResult();
        var errors = await Task.WhenAll(attempts);

        Assert.Single(errors, error => error is null);
        Assert.Single(
            errors,
            error => error is ExposureAuditConflictException);
        Assert.Single(sink.ExposureRecords);
    }

    private static ExposureAuditRecord ExposureRecord() =>
        new(
            "exposure-1",
            "decision-1",
            "tetris-demo",
            "dev",
            "2026-08-06T00:00:01Z",
            "2026-08-06T00:00:02Z");
}
