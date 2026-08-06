using System.Text.Json;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileAuditSinkTests
{
    [Fact]
    public async Task RecordsInspectableDecisionAndIdempotentExposureLines()
    {
        using var file = new TestJsonFile("audit");
        var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        var decision = DecisionRecord();
        var exposure = new ExposureAuditRecord(
            "exposure-1",
            decision.DecisionId,
            decision.AppId,
            decision.Environment,
            "2026-08-06T00:00:01Z",
            "2026-08-06T00:00:02Z");

        await sink.RecordDecisionAsync(decision, CancellationToken.None);
        await sink.RecordExposureAsync(exposure, CancellationToken.None);
        await sink.RecordExposureAsync(exposure, CancellationToken.None);
        using (var replaySink = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await replaySink.RecordExposureAsync(exposure, CancellationToken.None);
        }

        var lines = await File.ReadAllLinesAsync(file.Path);
        Assert.Equal(2, lines.Length);
        using var decisionLine = JsonDocument.Parse(lines[0]);
        using var exposureLine = JsonDocument.Parse(lines[1]);
        Assert.Equal("decision", decisionLine.RootElement.GetProperty("kind").GetString());
        Assert.Equal(
            "strategy-tetris-balanced-v1",
            decisionLine.RootElement.GetProperty("record").GetProperty("strategyId").GetString());
        Assert.Equal("exposure", exposureLine.RootElement.GetProperty("kind").GetString());
        Assert.Equal(
            "exposure-1",
            exposureLine.RootElement.GetProperty("record").GetProperty("exposureId").GetString());
        var persisted = await File.ReadAllTextAsync(file.Path);
        Assert.EndsWith("\n", persisted, StringComparison.Ordinal);
        Assert.DoesNotContain("\r\n", persisted, StringComparison.Ordinal);
        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task EmptyFile_IsAvailableAndAcceptsAppend()
    {
        using var file = new TestJsonFile("audit-empty");
        await file.WriteAsync(string.Empty);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
        await sink.RecordDecisionAsync(DecisionRecord(), CancellationToken.None);

        Assert.Single(await File.ReadAllLinesAsync(file.Path));
    }

    [Fact]
    public async Task ValidUnterminatedRecord_IsUnavailableAndRejectsAppend()
    {
        using var file = new TestJsonFile("audit-unterminated");
        var existingContent = JsonSerializer.Serialize(
            new { kind = "decision", record = DecisionRecord() },
            new JsonSerializerOptions(JsonSerializerDefaults.Web));
        await file.WriteAsync(existingContent);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        var error = await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordExposureAsync(
                new ExposureAuditRecord(
                    "exposure-1",
                    "decision-1",
                    "tetris-demo",
                    "dev",
                    null,
                    "2026-08-06T00:00:02Z"),
                CancellationToken.None));

        Assert.Contains("terminating newline", error.Message, StringComparison.Ordinal);
        Assert.Equal(existingContent, await File.ReadAllTextAsync(file.Path));
    }

    [Fact]
    public async Task CrLfTerminatedRecords_AreValidAndExposureDedupContinuesAcrossAppend()
    {
        using var file = new TestJsonFile("audit-crlf");
        var decision = DecisionRecord();
        var firstExposure = new ExposureAuditRecord(
            "exposure-1",
            decision.DecisionId,
            decision.AppId,
            decision.Environment,
            null,
            "2026-08-06T00:00:02Z");
        using (var initialSink = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await initialSink.RecordDecisionAsync(decision, CancellationToken.None);
            await initialSink.RecordExposureAsync(firstExposure, CancellationToken.None);
        }

        var lfContent = (await File.ReadAllTextAsync(file.Path))
            .Replace("\r\n", "\n", StringComparison.Ordinal);
        await file.WriteAsync(lfContent.Replace("\n", "\r\n", StringComparison.Ordinal));
        using var replaySink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await replaySink.IsAvailableAsync(CancellationToken.None));
        await replaySink.RecordExposureAsync(firstExposure, CancellationToken.None);
        await replaySink.RecordExposureAsync(
            firstExposure with { ExposureId = "exposure-2" },
            CancellationToken.None);

        var lines = await File.ReadAllLinesAsync(file.Path);
        Assert.Equal(3, lines.Length);
        Assert.Equal(
            ["exposure-1", "exposure-2"],
            lines.Skip(1)
                .Select(line => JsonDocument.Parse(line))
                .Select(document =>
                {
                    using (document)
                    {
                        return document.RootElement
                            .GetProperty("record")
                            .GetProperty("exposureId")
                            .GetString();
                    }
                }));
    }

    [Fact]
    public async Task ConcurrentSeparateInstances_RecordSameExposureIdExactlyOnce()
    {
        using var file = new TestJsonFile("audit-concurrent-same");
        var options = new LocalFileAuditSinkOptions(
            file.Path,
            TimeSpan.FromSeconds(30),
            TimeSpan.FromMilliseconds(5));
        using var first = new LocalFileAuditSink(options);
        using var second = new LocalFileAuditSink(options);
        var exposure = new ExposureAuditRecord(
            "exposure-race",
            "decision-race",
            "tetris-demo",
            "dev",
            null,
            "2026-08-06T00:00:02Z");
        var start = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var tasks = Enumerable.Range(0, 32)
            .Select(index => Task.Run(async () =>
            {
                await start.Task;
                var sink = index % 2 == 0 ? first : second;
                await sink.RecordExposureAsync(exposure, CancellationToken.None);
            }))
            .ToArray();

        start.SetResult();
        await Task.WhenAll(tasks);

        var lines = await File.ReadAllLinesAsync(file.Path);
        Assert.Single(lines);
        Assert.Equal(["exposure-race"], ExposureIds(lines));
    }

    [Fact]
    public async Task ConcurrentSeparateInstances_PreserveDistinctExposureIdsAndJsonLines()
    {
        using var file = new TestJsonFile("audit-concurrent-distinct");
        var options = new LocalFileAuditSinkOptions(
            file.Path,
            TimeSpan.FromSeconds(30),
            TimeSpan.FromMilliseconds(5));
        using var first = new LocalFileAuditSink(options);
        using var second = new LocalFileAuditSink(options);
        var expectedIds = Enumerable.Range(0, 32)
            .Select(index => $"exposure-race-{index:D2}")
            .ToArray();
        var start = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var tasks = expectedIds
            .Select((exposureId, index) => Task.Run(async () =>
            {
                await start.Task;
                var sink = index % 2 == 0 ? first : second;
                await sink.RecordExposureAsync(
                    new ExposureAuditRecord(
                        exposureId,
                        $"decision-race-{index:D2}",
                        "tetris-demo",
                        "dev",
                        null,
                        "2026-08-06T00:00:02Z"),
                    CancellationToken.None);
            }))
            .ToArray();

        start.SetResult();
        await Task.WhenAll(tasks);

        var lines = await File.ReadAllLinesAsync(file.Path);
        Assert.Equal(expectedIds.Length, lines.Length);
        Assert.Equal(
            expectedIds,
            ExposureIds(lines).Order(StringComparer.Ordinal).ToArray());
    }

    [Fact]
    public async Task SidecarLeaseContention_TimesOutExplicitly()
    {
        using var file = new TestJsonFile("audit-lease-timeout");
        await using var heldLease = new FileStream(
            $"{file.Path}.lock",
            FileMode.OpenOrCreate,
            FileAccess.ReadWrite,
            FileShare.None);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                TimeSpan.FromMilliseconds(100),
                TimeSpan.FromMilliseconds(10)));

        var error = await Assert.ThrowsAsync<TimeoutException>(
            () => sink.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));

        Assert.Contains($"{file.Path}.lock", error.Message, StringComparison.Ordinal);
        Assert.False(File.Exists(file.Path));
    }

    [Fact]
    public async Task SidecarLeaseContention_HonorsCancellation()
    {
        using var file = new TestJsonFile("audit-lease-cancellation");
        await using var heldLease = new FileStream(
            $"{file.Path}.lock",
            FileMode.OpenOrCreate,
            FileAccess.ReadWrite,
            FileShare.None);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                TimeSpan.FromSeconds(5),
                TimeSpan.FromMilliseconds(10)));
        using var cancellation = new CancellationTokenSource(
            TimeSpan.FromMilliseconds(100));

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => sink.RecordDecisionAsync(
                DecisionRecord(),
                cancellation.Token));
        Assert.False(File.Exists(file.Path));
    }

    [Fact]
    public async Task WriteFailure_IsSurfacedAndHealthIsUnavailable()
    {
        using var container = new TestJsonFile("audit-parent");
        await container.WriteAsync(string.Empty);
        var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                System.IO.Path.Combine(container.Path, "audit.jsonl")));

        await Assert.ThrowsAnyAsync<IOException>(
            () => sink.RecordDecisionAsync(DecisionRecord(), CancellationToken.None));
        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Theory]
    [InlineData("""{"kind":"exposure","record":{"exposureId":""}}""")]
    [InlineData("""{"kind":"decision","record":""")]
    public async Task MalformedExistingAuditFile_IsUnavailableAndRejectsAppend(
        string existingContent)
    {
        using var file = new TestJsonFile("audit-corrupt");
        await file.WriteAsync($"{existingContent}\n");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordExposureAsync(
                new ExposureAuditRecord(
                    "exposure-1",
                    "decision-1",
                    "tetris-demo",
                    "dev",
                    null,
                    "2026-08-06T00:00:02Z"),
                CancellationToken.None));
    }

    [Fact]
    public async Task ExposureConfirmation_CommitsOnlyAfterAppendAndReplaysWithoutDuplicates()
    {
        using var blockedParent = new TestJsonFile("audit-atomic-parent");
        await blockedParent.WriteAsync(string.Empty);
        var auditPath = System.IO.Path.Combine(blockedParent.Path, "audit.jsonl");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(auditPath));
        var store = new InMemoryExposureStore(
            new FixedTimeProvider(),
            () => "exposure-atomic");
        var service = new ExposureConfirmationService(store, sink);
        var request = new ExposureConfirmationRequest(
            "confirm-atomic",
            "2026-08-06T00:00:01Z");
        var appIds = new HashSet<string>(StringComparer.Ordinal) { "tetris-demo" };
        var environments = new HashSet<string>(StringComparer.Ordinal) { "dev" };
        await store.CreatePendingAsync(
            "decision-atomic",
            "confirm-atomic",
            Snapshot(),
            CancellationToken.None);

        await Assert.ThrowsAnyAsync<IOException>(
            () => service.ConfirmAsync(
                "decision-atomic",
                request,
                appIds,
                environments,
                CancellationToken.None));
        var preparedReplay = await store.FindReplayAsync(
            "decision-atomic",
            request,
            appIds,
            environments,
            CancellationToken.None);
        Assert.Equal("exposure-atomic", preparedReplay!.Result.ExposureId);
        Assert.Null(store.Find("decision-atomic")!.Confirmation);

        File.Delete(blockedParent.Path);
        Directory.CreateDirectory(blockedParent.Path);
        try
        {
            var first = await service.ConfirmAsync(
                "decision-atomic",
                request,
                appIds,
                environments,
                CancellationToken.None);
            var replay = await service.ConfirmAsync(
                "decision-atomic",
                request,
                appIds,
                environments,
                CancellationToken.None);

            Assert.Equal(first.Result, replay.Result);
            Assert.Single(await File.ReadAllLinesAsync(auditPath));
        }
        finally
        {
            Directory.Delete(blockedParent.Path, recursive: true);
        }
    }

    private static DecisionAuditRecord DecisionRecord() => new(
        "audit-1",
        "decision-1",
        "tetris.dropInterval",
        "tetris-demo",
        "dev",
        new RuntimeContractIdentity(
            "def-phase3",
            $"sha256:{new string('a', 64)}",
            "rev-phase3"),
        JsonSerializer.SerializeToElement(850),
        "number",
        "strategy",
        new ServerFallbackInfo("server", false, false, null),
        new Dictionary<string, JsonElement>(),
        [
            new SignalInput(
                new SignalRef("tetris.boardPressure"),
                JsonSerializer.SerializeToElement(0.9))
        ],
        new DecisionTargetRef("session", "game-1"),
        new DecisionTargetRef("cohort", "new_players"),
        [],
        ["session:game-1", "cohort:new_players", "global"],
        new PolicyEvaluationResult("approved", [], ["number-bounds", "max-delta"]),
        new DateTimeOffset(2026, 8, 6, 0, 0, 0, TimeSpan.Zero),
        Confidence: new ConfidenceReport(0.82, 0.2, 0.74),
        StrategyId: "strategy-tetris-balanced-v1");

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

    private static string[] ExposureIds(IEnumerable<string> lines) =>
        lines.Select(line =>
        {
            using var document = JsonDocument.Parse(line);
            Assert.Equal(
                "exposure",
                document.RootElement.GetProperty("kind").GetString());
            return document.RootElement
                .GetProperty("record")
                .GetProperty("exposureId")
                .GetString()!;
        }).ToArray();

    private sealed class FixedTimeProvider : TimeProvider
    {
        public override DateTimeOffset GetUtcNow() =>
            new(2026, 8, 6, 0, 0, 2, TimeSpan.Zero);
    }
}

internal sealed class TestJsonFile : IDisposable
{
    private readonly string _directory;

    public TestJsonFile(string prefix)
    {
        _directory = System.IO.Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts");
        Directory.CreateDirectory(_directory);
        Path = System.IO.Path.Combine(
            _directory,
            $"{prefix}-{Guid.NewGuid():N}.json");
    }

    public string Path { get; }

    public Task WriteAsync(string content) => File.WriteAllTextAsync(Path, content);

    public void Dispose()
    {
        if (File.Exists(Path))
        {
            File.Delete(Path);
        }

        var lockPath = $"{Path}.lock";
        if (File.Exists(lockPath))
        {
            File.Delete(lockPath);
        }
    }
}
