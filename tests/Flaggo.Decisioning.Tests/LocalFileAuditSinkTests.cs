using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Flaggo.Audit;
using Flaggo.Decisioning;
using Flaggo.Shared.Contracts;
using Flaggo.State;

namespace Flaggo.Decisioning.Tests;

public sealed class LocalFileAuditSinkTests
{
    public static TheoryData<string, ExposureAuditRecord>
        ConflictingExposureRecords()
    {
        var record = CanonicalExposureRecord();
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

        var lines = await ReadAuditLinesAsync(file.Path);
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
        var persisted = string.Join(
            "\n",
            await ReadAuditLinesAsync(file.Path)) + "\n";
        Assert.EndsWith("\n", persisted, StringComparison.Ordinal);
        Assert.DoesNotContain("\r\n", persisted, StringComparison.Ordinal);
        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task LegacyEmptyFile_FailsClosedWithoutMigration()
    {
        using var file = new TestJsonFile("audit-empty");
        await file.WriteAsync(string.Empty);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
        Assert.Equal(string.Empty, await File.ReadAllTextAsync(file.Path));
    }

    [Fact]
    public async Task ValidDecisionModeMatrix_RoundTrips()
    {
        using var file = new TestJsonFile("audit-valid-modes");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        var strategy = DecisionRecord();
        var records = new[]
        {
            strategy,
            strategy with
            {
                AuditId = "audit-experiment",
                DecisionId = "decision-experiment",
                DecisionMode = "experiment"
            },
            strategy with
            {
                AuditId = "audit-active",
                DecisionId = "decision-active",
                DecisionMode = "active-value",
                StrategyId = null,
                Evidence = null,
                Confidence = null
            },
            strategy with
            {
                AuditId = "audit-fallback-static",
                DecisionId = "decision-fallback-static",
                DecisionMode = "fallback",
                StrategyId = null,
                Evidence = null,
                Confidence = null,
                Fallback = new ServerFallbackInfo(
                    "server",
                    false,
                    true,
                    "fallback_required"),
                Policy = new PolicyEvaluationResult("fallback", [], [])
            },
            strategy with
            {
                AuditId = "audit-fallback-strategy",
                DecisionId = "decision-fallback-strategy",
                DecisionMode = "fallback",
                Confidence = null,
                Fallback = new ServerFallbackInfo(
                    "server",
                    false,
                    true,
                    "fallback_required"),
                Policy = new PolicyEvaluationResult(
                    "blocked",
                    ["minimum-evidence-quality"],
                    ["minimum-evidence-quality"])
            },
            strategy with
            {
                AuditId = "audit-fallback-strategy-no-evidence",
                DecisionId = "decision-fallback-strategy-no-evidence",
                DecisionMode = "fallback",
                Evidence = null,
                Confidence = null,
                Fallback = new ServerFallbackInfo(
                    "server",
                    false,
                    true,
                    "fallback_required"),
                Policy = new PolicyEvaluationResult(
                    "fallback",
                    ["strategy_confidence_unavailable"],
                    [])
            }
        };

        foreach (var record in records)
        {
            await sink.RecordDecisionAsync(record, CancellationToken.None);
        }

        Assert.Equal(records.Length, (await ReadAuditLinesAsync(file.Path)).Length);
        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task InvalidDecisionEvidenceMatrix_RejectsNewRecords()
    {
        var valid = DecisionRecord();
        var invalidRecords = new[]
        {
            valid with { Evidence = null },
            valid with
            {
                Confidence = valid.Confidence! with { EvidenceQuality = 0.81 }
            },
            valid with
            {
                Confidence = valid.Confidence! with { ModelUncertainty = null }
            },
            valid with
            {
                Evidence = valid.Evidence! with { SampleSize = -1 }
            },
            valid with
            {
                Evidence = valid.Evidence! with { EvidenceQuality = 1.01 }
            },
            valid with
            {
                Confidence = valid.Confidence! with { ExpectedOutcome = 1.01 }
            },
            valid with
            {
                DecisionMode = "active-value",
                StrategyId = null,
                Confidence = null
            },
            valid with
            {
                DecisionMode = "active-value",
                StrategyId = null,
                Evidence = null
            },
            valid with
            {
                DecisionMode = "fallback",
                StrategyId = null,
                Confidence = null,
                Fallback = new ServerFallbackInfo(
                    "server",
                    false,
                    true,
                    "fallback_required"),
                Policy = new PolicyEvaluationResult("fallback", [], [])
            }
        };

        foreach (var invalid in invalidRecords)
        {
            using var file = new TestJsonFile("audit-invalid-evidence-matrix");
            using var sink = new LocalFileAuditSink(
                new LocalFileAuditSinkOptions(file.Path));

            await Assert.ThrowsAsync<InvalidDataException>(
                () => sink.RecordDecisionAsync(invalid, CancellationToken.None));
            Assert.False(File.Exists(file.Path));
        }
    }

    [Fact]
    public async Task StrategyRecordWithoutEvidence_IsCorruptOnReplay()
    {
        using var file = new TestJsonFile("audit-strategy-missing-evidence");
        await ReplaceSegmentRecordsAsync(
            file.Path,
            [DecisionEnvelope(record => record.Remove("evidence"))]);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
    }

    public static TheoryData<string, string> CorruptEvidenceDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
            "evidence probability",
            DecisionEnvelope(
                record => record["evidence"]!["evidenceQuality"] = 1.01));
        cases.Add(
            "confidence probability",
            DecisionEnvelope(
                record => record["confidence"]!["modelUncertainty"] = -0.01));
        cases.Add(
            "sample size",
            DecisionEnvelope(
                record => record["evidence"]!["sampleSize"] = -1));
        cases.Add(
            "evidence confidence mismatch",
            DecisionEnvelope(
                record => record["confidence"]!["expectedOutcome"] = 0.75));
        return cases;
    }

    [Theory]
    [MemberData(nameof(CorruptEvidenceDocuments))]
    public async Task CorruptEvidenceOrConfidence_IsRejectedOnReplay(
        string name,
        string document)
    {
        _ = name;
        using var file = new TestJsonFile("audit-corrupt-evidence");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
    }

    [Fact]
    public async Task ValidUnterminatedRecord_IsUnavailableAndRejectsAppend()
    {
        using var file = new TestJsonFile("audit-unterminated");
        var existingContent = JsonSerializer.Serialize(
            new { kind = "decision", record = DecisionRecord() },
            new JsonSerializerOptions(JsonSerializerDefaults.Web));
        await ReplaceSegmentRecordsAsync(
            file.Path,
            [existingContent],
            terminate: false);
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
    }

    [Fact]
    public async Task CrLfTerminatedRecords_AreValidAndExposureDedupContinuesAcrossAppend()
    {
        using var file = new TestJsonFile("audit-crlf");
        var decision = DecisionRecord();
        var options = new JsonSerializerOptions(JsonSerializerDefaults.Web)
        {
            DefaultIgnoreCondition =
                System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull
        };
        await ReplaceSegmentRecordsAsync(
            file.Path,
            [
                JsonSerializer.Serialize(
                    new { kind = "decision", record = decision },
                    options)
            ],
            "\r\n");
        using var replaySink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await replaySink.IsAvailableAsync(CancellationToken.None));
        await replaySink.RecordDecisionAsync(
            decision with
            {
                AuditId = "audit-2",
                DecisionId = "decision-2"
            },
            CancellationToken.None);

        var lines = await ReadAuditLinesAsync(file.Path);
        Assert.Equal(2, lines.Length);
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

        var lines = await ReadAuditLinesAsync(file.Path);
        Assert.Single(lines);
        Assert.Equal(["exposure-race"], ExposureIds(lines));
    }

    [Theory]
    [MemberData(nameof(ConflictingExposureRecords))]
    public async Task ExistingExposureIdentity_ConflictingFieldFailsClosed(
        string field,
        ExposureAuditRecord conflict)
    {
        _ = field;
        using var file = new TestJsonFile("audit-exposure-conflict");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        await sink.RecordExposureAsync(
            CanonicalExposureRecord(),
            CancellationToken.None);

        var error = await Assert.ThrowsAsync<ExposureAuditConflictException>(
            () => sink.RecordExposureAsync(conflict, CancellationToken.None));

        Assert.Equal("exposure-1", error.ExposureId);
        Assert.Single(await ReadAuditLinesAsync(file.Path));
    }

    [Fact]
    public async Task ConcurrentConflictingExposureIdentity_HasOneWinner()
    {
        using var file = new TestJsonFile("audit-exposure-conflict-race");
        var options = new LocalFileAuditSinkOptions(
            file.Path,
            TimeSpan.FromSeconds(30),
            TimeSpan.FromMilliseconds(5));
        using var firstSink = new LocalFileAuditSink(options);
        using var secondSink = new LocalFileAuditSink(options);
        var first = CanonicalExposureRecord();
        var second = first with { DecisionId = "decision-2" };
        var start = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var attempts = new[]
        {
            (Sink: firstSink, Record: first),
            (Sink: secondSink, Record: second)
        }.Select(item => Task.Run(async () =>
        {
            await start.Task;
            return await Record.ExceptionAsync(
                () => item.Sink.RecordExposureAsync(
                    item.Record,
                    CancellationToken.None));
        })).ToArray();

        start.SetResult();
        var errors = await Task.WhenAll(attempts);

        Assert.Single(errors, error => error is null);
        Assert.Single(
            errors,
            error => error is ExposureAuditConflictException);
        Assert.Single(await ReadAuditLinesAsync(file.Path));
    }

    [Fact]
    public async Task NestedMissingAuditParent_IsDurableBeforeLeaseAndLayout()
    {
        var root = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"audit-nested-parent-{Guid.NewGuid():N}");
        var auditPath = Path.Combine(root, "one", "two", "audit.jsonl");
        var operations = new RecordingAuditDirectoryOperations();
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(auditPath),
            operations);
        try
        {
            await sink.RecordExposureAsync(
                CanonicalExposureRecord(),
                CancellationToken.None);

            var firstMissing = Path.Combine(root, "one");
            var durableParent = Path.GetDirectoryName(firstMissing)!;
            Assert.True(
                operations.Events.IndexOf($"mkdir:{firstMissing}") <
                operations.Events.LastIndexOf($"sync:{durableParent}"));
            Assert.True(File.Exists($"{auditPath}.lock"));
            Assert.True(Directory.Exists($"{auditPath}.d"));
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }
        }
    }

    [Fact]
    public async Task NestedMissingAuditParentSyncFailure_PreventsLeaseAndLayout()
    {
        var root = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"audit-nested-failure-{Guid.NewGuid():N}");
        var auditPath = Path.Combine(root, "one", "two", "audit.jsonl");
        var operations = new RecordingAuditDirectoryOperations
        {
            FailAfterFirstCreate = true
        };
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(auditPath),
            operations);
        try
        {
            await Assert.ThrowsAsync<IOException>(
                () => sink.RecordExposureAsync(
                    CanonicalExposureRecord(),
                    CancellationToken.None));

            Assert.False(File.Exists($"{auditPath}.lock"));
            Assert.False(Directory.Exists($"{auditPath}.d"));
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }
        }
    }

    [Fact]
    public async Task ConcurrentCreators_DurablyCreateNestedAuditParent()
    {
        var root = Path.Combine(
            TestPaths.RepositoryRoot,
            ".flaggo",
            "test-artifacts",
            $"audit-nested-race-{Guid.NewGuid():N}");
        var auditPath = Path.Combine(root, "one", "two", "audit.jsonl");
        var operations = new RecordingAuditDirectoryOperations();
        var options = new LocalFileAuditSinkOptions(
            auditPath,
            TimeSpan.FromSeconds(30),
            TimeSpan.FromMilliseconds(5));
        using var first = new LocalFileAuditSink(options, operations);
        using var second = new LocalFileAuditSink(options, operations);
        try
        {
            await Task.WhenAll(
                first.RecordExposureAsync(
                    CanonicalExposureRecord(),
                    CancellationToken.None),
                second.RecordExposureAsync(
                    CanonicalExposureRecord() with
                    {
                        ExposureId = "exposure-2",
                        DecisionId = "decision-2"
                    },
                    CancellationToken.None));

            Assert.Equal(2, (await ReadAuditLinesAsync(auditPath)).Length);
            Assert.True(Directory.Exists(Path.GetDirectoryName(auditPath)));
        }
        finally
        {
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }
        }
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

        var lines = await ReadAuditLinesAsync(file.Path);
        Assert.Equal(expectedIds.Length, lines.Length);
        Assert.Equal(
            expectedIds,
            ExposureIds(lines).Order(StringComparer.Ordinal).ToArray());
    }

    [Fact]
    public async Task SegmentRotation_BoundsEveryAppendScanAndSegment()
    {
        using var file = new TestJsonFile("audit-segment-rotation");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 3,
                MaximumSegmentBytes: 64 * 1024));

        for (var index = 0; index < 10; index++)
        {
            await sink.RecordDecisionAsync(
                DecisionRecord() with
                {
                    AuditId = $"audit-{index}",
                    DecisionId = $"decision-{index}"
                },
                CancellationToken.None);
            Assert.InRange(sink.LastAppendValidatedRecordCount, 0, 3);
        }

        var segments = SegmentPaths(file.Path);
        Assert.Equal(4, segments.Length);
        Assert.All(
            segments,
            segment =>
            {
                Assert.InRange(File.ReadAllLines(segment).Length - 1, 1, 3);
                Assert.InRange(new FileInfo(segment).Length, 1, 64 * 1024);
            });
        Assert.Equal(10, (await ReadAuditLinesAsync(file.Path)).Length);
    }

    [Fact]
    public async Task Readiness_DetectsCorruptionInArbitraryOldSegment()
    {
        using var file = new TestJsonFile("audit-old-segment-corruption");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(
                       file.Path,
                       MaximumSegmentRecords: 1)))
        {
            for (var index = 0; index < 3; index++)
            {
                await writer.RecordDecisionAsync(
                    DecisionRecord() with
                    {
                        AuditId = $"audit-{index}",
                        DecisionId = $"decision-{index}"
                    },
                    CancellationToken.None);
            }
        }

        var oldest = SegmentPaths(file.Path).First();
        var lines = await File.ReadAllLinesAsync(oldest);
        lines[1] = "{corrupt";
        await File.WriteAllLinesAsync(oldest, lines);

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 1));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task Readiness_RebuildsOnlyMissingExposureMarker()
    {
        using var file = new TestJsonFile("audit-marker-rebuild");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordExposureAsync(
                ExposureRecord("exposure-indexed"),
                CancellationToken.None);
        }

        var marker = Assert.Single(MarkerPaths(file.Path));
        File.Delete(marker);

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await restarted.IsAvailableAsync(CancellationToken.None));
        Assert.Single(MarkerPaths(file.Path));
        Assert.Contains(
            "exposure-indexed",
            await File.ReadAllTextAsync(Assert.Single(MarkerPaths(file.Path))),
            StringComparison.Ordinal);
    }

    [Fact]
    public async Task CorruptExposureMarker_FailsClosedWithoutRepair()
    {
        using var file = new TestJsonFile("audit-marker-corrupt");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordExposureAsync(
                ExposureRecord("exposure-corrupt"),
                CancellationToken.None);
        }
        var marker = Assert.Single(MarkerPaths(file.Path));
        await File.WriteAllTextAsync(marker, "{corrupt");

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => restarted.RecordExposureAsync(
                ExposureRecord("exposure-corrupt"),
                CancellationToken.None));
        Assert.Equal("{corrupt", await File.ReadAllTextAsync(marker));
    }

    [Fact]
    public async Task CrashAfterRecordBeforeMarker_RecoversWithoutDuplicate()
    {
        using var file = new TestJsonFile("audit-record-before-marker");
        using (var crashing = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            crashing.AfterRecordDurablyFlushed = () =>
                throw new SimulatedCrashException();
            await Assert.ThrowsAsync<SimulatedCrashException>(
                () => crashing.RecordExposureAsync(
                    ExposureRecord("exposure-crash"),
                    CancellationToken.None));
        }
        Assert.Empty(MarkerPaths(file.Path));
        Assert.Single(await ReadAuditLinesAsync(file.Path));

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        await restarted.RecordExposureAsync(
            ExposureRecord("exposure-crash"),
            CancellationToken.None);

        Assert.Single(await ReadAuditLinesAsync(file.Path));
        Assert.Single(MarkerPaths(file.Path));
        Assert.True(await restarted.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task Restart_PreservesDurableSegmentAndMarkerState()
    {
        using var file = new TestJsonFile("audit-restart");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordExposureAsync(
                ExposureRecord("exposure-restart"),
                CancellationToken.None);
        }

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        await restarted.RecordExposureAsync(
            ExposureRecord("exposure-restart"),
            CancellationToken.None);

        Assert.Single(await ReadAuditLinesAsync(file.Path));
        Assert.Single(MarkerPaths(file.Path));
        Assert.True(await restarted.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task Readiness_RejectsMarkerThatClaimsNoAuditRecord()
    {
        using var file = new TestJsonFile("audit-orphan-marker");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
        }
        var markerDirectory = System.IO.Path.Combine(
            $"{file.Path}.d",
            "exposures");
        await File.WriteAllTextAsync(
            System.IO.Path.Combine(markerDirectory, $"{new string('a', 64)}.json"),
            """{"version":1,"exposureId":"phantom","segmentId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","segmentSequence":1,"recordIndex":0,"recordHash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}""");

        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task FreshInitialization_PublishesExplicitValidatedManifest()
    {
        using var file = new TestJsonFile("audit-fresh-manifest");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));

        var manifest = await ReadManifestAsync(file.Path);
        Assert.Equal(1, manifest["version"]!.GetValue<int>());
        Assert.Equal(1, manifest["generation"]!.GetValue<long>());
        var segment = Assert.Single(manifest["segments"]!.AsArray());
        Assert.True(segment!["current"]!.GetValue<bool>());
        Assert.Equal(0, segment["recordCount"]!.GetValue<int>());
    }

    [Fact]
    public async Task ListedCurrentSegmentLoss_IsCorruptionAndIsNeverRecreated()
    {
        using var file = new TestJsonFile("audit-current-loss");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
        }
        var currentPath = await CurrentSegmentPathAsync(file.Path);
        File.Delete(currentPath);

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => restarted.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
        Assert.False(File.Exists(currentPath));
    }

    [Fact]
    public async Task UnlistedSegment_FailsReadinessAndAppend()
    {
        using var file = new TestJsonFile("audit-unlisted-segment");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
        }
        var currentPath = await CurrentSegmentPathAsync(file.Path);
        var extraPath = System.IO.Path.Combine(
            System.IO.Path.GetDirectoryName(currentPath)!,
            $"segment-{2L:D20}-{new string('a', 32)}.jsonl");
        File.Copy(currentPath, extraPath);

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => restarted.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
        Assert.True(File.Exists(extraPath));
    }

    [Fact]
    public async Task CorruptManifest_IsNotRebuiltFromSegments()
    {
        using var file = new TestJsonFile("audit-manifest-corrupt");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
        }
        var manifestPath = System.IO.Path.Combine(
            $"{file.Path}.d",
            "manifest.json");
        await File.WriteAllTextAsync(manifestPath, "{corrupt");

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => restarted.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));
        Assert.Equal("{corrupt", await File.ReadAllTextAsync(manifestPath));
    }

    [Fact]
    public async Task InterruptedInitialization_RecoversOnlyUnpublishedStaging()
    {
        using var recoverable = new TestJsonFile("audit-init-staging");
        var staging = $"{recoverable.Path}.d.tmp-{Guid.NewGuid():N}";
        Directory.CreateDirectory(staging);
        await File.WriteAllTextAsync(
            System.IO.Path.Combine(staging, "partial"),
            "partial");
        using (var sink = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(recoverable.Path)))
        {
            Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
        }
        Assert.False(Directory.Exists(staging));

        using var ambiguous = new TestJsonFile("audit-init-ambiguous");
        Directory.CreateDirectory(
            System.IO.Path.Combine($"{ambiguous.Path}.d", "segments"));
        Directory.CreateDirectory(
            System.IO.Path.Combine($"{ambiguous.Path}.d", "exposures"));
        using var ambiguousSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(ambiguous.Path));
        Assert.False(
            await ambiguousSink.IsAvailableAsync(CancellationToken.None));
        Assert.False(
            File.Exists(System.IO.Path.Combine(
                $"{ambiguous.Path}.d",
                "manifest.json")));
    }

    [Fact]
    public async Task InterruptedRotationBeforeManifest_FailsClosedOnUnlistedSegment()
    {
        using var file = new TestJsonFile("audit-rotation-before-manifest");
        using var writer = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 1));
        await writer.RecordDecisionAsync(
            DecisionRecord(),
            CancellationToken.None);
        writer.AfterNewSegmentDurablyFlushed = () =>
            throw new SimulatedCrashException();

        await Assert.ThrowsAsync<SimulatedCrashException>(
            () => writer.RecordDecisionAsync(
                DecisionRecord() with
                {
                    AuditId = "audit-2",
                    DecisionId = "decision-2"
                },
                CancellationToken.None));

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 1));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        Assert.Equal(2, SegmentPaths(file.Path).Length);
        Assert.Single((await ReadManifestAsync(file.Path))["segments"]!.AsArray());
    }

    [Fact]
    public async Task InterruptedRotationAfterManifest_ExposesOnlyDurableGeneration()
    {
        using var file = new TestJsonFile("audit-rotation-after-manifest");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(
                       file.Path,
                       MaximumSegmentRecords: 1)))
        {
            await writer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
            writer.AfterRotationManifestDurablyFlushed = () =>
                throw new SimulatedCrashException();
            await Assert.ThrowsAsync<SimulatedCrashException>(
                () => writer.RecordDecisionAsync(
                    DecisionRecord() with
                    {
                        AuditId = "audit-2",
                        DecisionId = "decision-2"
                    },
                    CancellationToken.None));
        }

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 1));
        Assert.True(await restarted.IsAvailableAsync(CancellationToken.None));
        Assert.Single(await ReadAuditLinesAsync(file.Path));
        await restarted.RecordDecisionAsync(
            DecisionRecord() with
            {
                AuditId = "audit-2",
                DecisionId = "decision-2"
            },
            CancellationToken.None);
        Assert.Equal(2, (await ReadAuditLinesAsync(file.Path)).Length);
    }

    [Theory]
    [InlineData("missing-segment")]
    [InlineData("wrong-record")]
    [InlineData("wrong-hash")]
    [InlineData("other-exposure")]
    public async Task InvalidExposureMarkerReference_FailsClosed(string mutation)
    {
        using var file = new TestJsonFile("audit-marker-reference");
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordExposureAsync(
                ExposureRecord("exposure-one"),
                CancellationToken.None);
            await writer.RecordExposureAsync(
                ExposureRecord("exposure-two"),
                CancellationToken.None);
        }
        var markers = await Task.WhenAll(
            MarkerPaths(file.Path).Select(async path =>
                (Path: path,
                 Node: JsonNode.Parse(await File.ReadAllTextAsync(path))!
                     .AsObject())));
        var target = markers.Single(item =>
            item.Node["exposureId"]!.GetValue<string>() == "exposure-one");
        var other = markers.Single(item =>
            item.Node["exposureId"]!.GetValue<string>() == "exposure-two");
        switch (mutation)
        {
            case "missing-segment":
                target.Node["segmentSequence"] = 99;
                target.Node["segmentId"] = new string('a', 32);
                break;
            case "wrong-record":
                target.Node["recordIndex"] =
                    other.Node["recordIndex"]!.GetValue<int>();
                target.Node["recordHash"] =
                    other.Node["recordHash"]!.GetValue<string>();
                break;
            case "wrong-hash":
                target.Node["recordHash"] =
                    $"sha256:{new string('a', 64)}";
                break;
            case "other-exposure":
                target.Node["exposureId"] = "exposure-two";
                break;
        }
        await File.WriteAllTextAsync(target.Path, target.Node.ToJsonString());

        using var restarted = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.False(await restarted.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => restarted.RecordExposureAsync(
                ExposureRecord("exposure-one"),
                CancellationToken.None));
        Assert.Equal(2, (await ReadAuditLinesAsync(file.Path)).Length);
    }

    [Theory]
    [InlineData("""{"value":1,"value":2}""")]
    [InlineData("""{"nested":{"value":1,"value":2}}""")]
    public async Task EvidenceDetailsDuplicateProperties_AreRejectedBeforeAppend(
        string rawDetails)
    {
        using var file = new TestJsonFile("audit-details-duplicate");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        var invalid = DecisionRecord() with
        {
            Evidence = DecisionRecord().Evidence! with
            {
                Details = new Dictionary<string, JsonElement>
                {
                    ["test"] = JsonValue(rawDetails)
                }
            }
        };

        await Assert.ThrowsAsync<InvalidDataException>(
            () => sink.RecordDecisionAsync(invalid, CancellationToken.None));
        Assert.False(Directory.Exists($"{file.Path}.d"));
    }

    [Fact]
    public async Task AcceptedSerializedRecords_RoundTripThroughReplayValidation()
    {
        using var file = new TestJsonFile("audit-roundtrip-property");
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(
                file.Path,
                MaximumSegmentRecords: 17));
        var random = new Random(1949);
        for (var index = 0; index < 100; index++)
        {
            var value = random.Next(-1_000_000, 1_000_001) / 100d;
            await sink.RecordDecisionAsync(
                DecisionRecord() with
                {
                    AuditId = $"audit-{index}",
                    DecisionId = $"decision-{index}",
                    Value = JsonSerializer.SerializeToElement(value),
                    RuntimeContext = new Dictionary<string, JsonElement>
                    {
                        ["value"] = JsonSerializer.SerializeToElement(value),
                        ["iteration"] = JsonSerializer.SerializeToElement(index)
                    }
                },
                CancellationToken.None);
        }

        Assert.Equal(100, (await ReadAuditLinesAsync(file.Path)).Length);
        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
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
        await ReplaceSegmentRecordsAsync(file.Path, [existingContent]);
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

    public static TheoryData<string, string> InvalidRecordedAtDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
                "missing",
                DecisionEnvelope(record => record.Remove("recordedAt")));
        cases.Add(
                "malformed",
                DecisionEnvelope(record => record["recordedAt"] = "not-a-time"));
        cases.Add(
                "missing offset",
                DecisionEnvelope(
                    record => record["recordedAt"] = "2026-08-06T00:00:00"));
        cases.Add(
                "default",
                DecisionEnvelope(
                    record => record["recordedAt"] =
                        "0001-01-01T00:00:00+00:00"));
        foreach (var (name, contamination) in StrictStringContaminations())
        {
            cases.Add(
                $"recordedAt {name} prefix",
                DecisionEnvelope(
                    record => record["recordedAt"] =
                        $"{contamination}2026-08-06T00:00:00Z"));
            cases.Add(
                $"recordedAt {name} suffix",
                DecisionEnvelope(
                    record => record["recordedAt"] =
                        $"2026-08-06T00:00:00Z{contamination}"));
        }
        return cases;
    }

    [Theory]
    [MemberData(nameof(InvalidRecordedAtDocuments))]
    public async Task InvalidOrMissingRecordedAt_MakesExistingAuditUnavailable(
        string name,
        string document)
    {
        _ = name;
        using var file = new TestJsonFile("audit-recorded-at-invalid");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
                new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
                () => sink.RecordDecisionAsync(
                    DecisionRecord(),
                    CancellationToken.None));
    }

    public static TheoryData<DateTimeOffset> BoundaryRecordedAtValues() =>
        new()
        {
                DateTimeOffset.MinValue.AddTicks(1),
                DateTimeOffset.MaxValue
        };

    [Theory]
    [MemberData(nameof(BoundaryRecordedAtValues))]
    public async Task LegitimateBoundaryRecordedAtValues_RoundTrip(
        DateTimeOffset recordedAt)
    {
        using var file = new TestJsonFile("audit-recorded-at-boundary");
        using (var writer = new LocalFileAuditSink(
                       new LocalFileAuditSinkOptions(file.Path)))
        {
                await writer.RecordDecisionAsync(
                    DecisionRecord() with { RecordedAt = recordedAt },
                    CancellationToken.None);
        }
        using var reader = new LocalFileAuditSink(
                new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await reader.IsAvailableAsync(CancellationToken.None));
    }

    [Theory]
    [InlineData("2026-08-06T00:00:00Z")]
    [InlineData("2026-08-06T00:00:00+14:00")]
    [InlineData("2026-08-06T00:00:00-14:00")]
    public async Task ExactDigestAndTimestampStringsRemainValid(string recordedAt)
    {
        var digest = $"sha256:{new string('a', 64)}";
        var document = DecisionEnvelope(
            record =>
            {
                record["recordedAt"] = recordedAt;
                record["contract"]!["contractDigest"] = digest;
                record["contract"]!["bundleDigest"] = digest;
            });
        using var file = new TestJsonFile("audit-exact-strict-strings");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.True(await sink.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string, string> MissingDefaultableDecisionFields()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
                "resolution fallback flag",
                DecisionEnvelope(record =>
                    record["fallback"]!.AsObject()
                        .Remove("resolutionFallbackUsed")));
        cases.Add(
                "decision fallback flag",
                DecisionEnvelope(record =>
                    record["fallback"]!.AsObject()
                        .Remove("decisionFallbackUsed")));
        cases.Add(
                "confidence evidence quality",
                DecisionEnvelope(record =>
                    record["confidence"]!.AsObject()
                        .Remove("evidenceQuality")));
        cases.Add(
                "evidence evidence quality",
                DecisionEnvelope(record =>
                    record["evidence"]!.AsObject()
                        .Remove("evidenceQuality")));
        return cases;
    }

    [Theory]
    [MemberData(nameof(MissingDefaultableDecisionFields))]
    public async Task MissingDefaultableDecisionField_IsRejected(
        string name,
        string document)
    {
        _ = name;
        using var file = new TestJsonFile("audit-required-default");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
                new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string> MalformedContractDigests()
    {
        var cases = new TheoryData<string>
        {
            "sha256:abc",
            "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "md5:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        };
        var valid = $"sha256:{new string('a', 64)}";
        foreach (var (_, contamination) in StrictStringContaminations())
        {
            cases.Add($"{contamination}{valid}");
            cases.Add($"{valid}{contamination}");
        }

        return cases;
    }

    [Theory]
    [MemberData(nameof(MalformedContractDigests))]
    public async Task MalformedContractDigest_RejectsReplayAndNewAppend(
        string contractDigest)
    {
        using var existingFile = new TestJsonFile("audit-contract-digest-existing");
        await ReplaceSegmentRecordsAsync(
            existingFile.Path,
            [DecisionEnvelope(record =>
                record["contract"]!["contractDigest"] = contractDigest)]);
        using var existingSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(existingFile.Path));

        Assert.False(await existingSink.IsAvailableAsync(CancellationToken.None));
        await Assert.ThrowsAsync<InvalidDataException>(
            () => existingSink.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None));

        using var newFile = new TestJsonFile("audit-contract-digest-new");
        using var newSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(newFile.Path));
        var invalid = DecisionRecord() with
        {
            Contract = DecisionRecord().Contract with
            {
                ContractDigest = contractDigest
            }
        };

        await Assert.ThrowsAsync<InvalidDataException>(
            () => newSink.RecordDecisionAsync(invalid, CancellationToken.None));
        Assert.False(File.Exists(newFile.Path));
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{}")]
    [InlineData("[]")]
    public async Task NonPrimitiveRuntimeContext_RejectsReplayAndNewAppend(
        string rawValue)
    {
        using var existingFile = new TestJsonFile("audit-context-existing");
        await ReplaceSegmentRecordsAsync(
            existingFile.Path,
            [DecisionEnvelope(record =>
                record["runtimeContext"]!["invalid"] = JsonNode.Parse(rawValue))]);
        using var existingSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(existingFile.Path));

        Assert.False(await existingSink.IsAvailableAsync(CancellationToken.None));

        using var newFile = new TestJsonFile("audit-context-new");
        using var newSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(newFile.Path));
        var invalid = DecisionRecord() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["invalid"] = JsonValue(rawValue)
            }
        };

        await Assert.ThrowsAsync<InvalidDataException>(
            () => newSink.RecordDecisionAsync(invalid, CancellationToken.None));
        Assert.False(File.Exists(newFile.Path));
    }

    [Theory]
    [InlineData("1e400")]
    [InlineData("9007199254740993")]
    public async Task NonfiniteOrUnsafeRuntimeNumber_RejectsReplayAndNewAppend(
        string rawValue)
    {
        using var existingFile = new TestJsonFile("audit-number-existing");
        await ReplaceSegmentRecordsAsync(
            existingFile.Path,
            [DecisionEnvelope(record =>
                record["inputs"]![0]!["value"] = JsonNode.Parse(rawValue))]);
        using var existingSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(existingFile.Path));

        Assert.False(await existingSink.IsAvailableAsync(CancellationToken.None));

        using var newFile = new TestJsonFile("audit-number-new");
        using var newSink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(newFile.Path));
        var invalid = DecisionRecord() with
        {
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["invalidNumber"] = JsonValue(rawValue)
            }
        };

        await Assert.ThrowsAsync<InvalidDataException>(
            () => newSink.RecordDecisionAsync(invalid, CancellationToken.None));
        Assert.False(File.Exists(newFile.Path));
    }

    public static TheoryData<string, string> InvalidEmbeddedDecisionDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
            "unsafe decision value",
            DecisionEnvelope(record =>
                record["value"] = JsonNode.Parse("9007199254740993")));
        cases.Add(
            "malformed optional bundle digest",
            DecisionEnvelope(record =>
                record["contract"]!["bundleDigest"] = "sha256:ABC"));
        cases.Add(
            "unknown fallback source",
            DecisionEnvelope(record =>
                record["fallback"]!["source"] = "client"));
        cases.Add(
            "unknown policy result",
            DecisionEnvelope(record =>
                record["policy"]!["result"] = "unknown"));
        cases.Add(
            "unknown target provenance source",
            DecisionEnvelope(record =>
            {
                record["targetProvenance"] = new JsonArray(
                    JsonSerializer.SerializeToNode(
                        new TargetResolutionProvenance(
                            "cohort",
                            "new_players",
                            "unknown")));
            }));
        cases.Add(
            "duplicate signal input",
            DecisionEnvelope(record =>
            {
                var inputs = record["inputs"]!.AsArray();
                inputs.Add(inputs[0]!.DeepClone());
            }));
        return cases;
    }

    [Theory]
    [MemberData(nameof(InvalidEmbeddedDecisionDocuments))]
    public async Task InvalidEmbeddedDecisionInvariant_IsRejected(
        string name,
        string document)
    {
        _ = name;
        using var file = new TestJsonFile("audit-embedded-invalid");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
    }

    [Fact]
    public async Task Ieee754BoundaryPrimitives_RoundTrip()
    {
        using var file = new TestJsonFile("audit-ieee-boundary");
        var positiveBoundary = JsonValue("9007199254740992");
        var negativeBoundary = JsonValue("-9007199254740992");
        var record = DecisionRecord() with
        {
            Value = positiveBoundary,
            RuntimeContext = new Dictionary<string, JsonElement>
            {
                ["positiveBoundary"] = positiveBoundary,
                ["negativeBoundary"] = negativeBoundary,
                ["enabled"] = JsonSerializer.SerializeToElement(true),
                ["cohort"] = JsonSerializer.SerializeToElement("new_players")
            },
            Inputs =
            [
                new SignalInput(
                    new SignalRef("tetris.boardPressure"),
                    negativeBoundary)
            ]
        };
        using (var writer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(file.Path)))
        {
            await writer.RecordDecisionAsync(record, CancellationToken.None);
        }

        using var reader = new LocalFileAuditSink(
            new LocalFileAuditSinkOptions(file.Path));
        Assert.True(await reader.IsAvailableAsync(CancellationToken.None));
    }

    public static TheoryData<string, string> InvalidExposureTimestampDocuments()
    {
        var cases = new TheoryData<string, string>();
        cases.Add(
                "default confirmedAt",
                ExposureEnvelope(record =>
                    record["confirmedAt"] = "0001-01-01T00:00:00+00:00"));
        cases.Add(
                "confirmedAt without offset",
                ExposureEnvelope(record =>
                    record["confirmedAt"] = "2026-08-06T00:00:02"));
        cases.Add(
                "default appliedAt",
                ExposureEnvelope(record =>
                    record["appliedAt"] = "0001-01-01T00:00:00+00:00"));
        cases.Add(
                "appliedAt without offset",
                ExposureEnvelope(record =>
                    record["appliedAt"] = "2026-08-06T00:00:01"));
        foreach (var (name, contamination) in StrictStringContaminations())
        {
            cases.Add(
                $"confirmedAt {name} prefix",
                ExposureEnvelope(
                    record => record["confirmedAt"] =
                        $"{contamination}2026-08-06T00:00:02Z"));
            cases.Add(
                $"confirmedAt {name} suffix",
                ExposureEnvelope(
                    record => record["confirmedAt"] =
                        $"2026-08-06T00:00:02Z{contamination}"));
            cases.Add(
                $"appliedAt {name} prefix",
                ExposureEnvelope(
                    record => record["appliedAt"] =
                        $"{contamination}2026-08-06T00:00:01Z"));
            cases.Add(
                $"appliedAt {name} suffix",
                ExposureEnvelope(
                    record => record["appliedAt"] =
                        $"2026-08-06T00:00:01Z{contamination}"));
        }
        return cases;
    }

    [Theory]
    [MemberData(nameof(InvalidExposureTimestampDocuments))]
    public async Task InvalidExposureTimestamp_IsRejected(
        string name,
        string document)
    {
        _ = name;
        using var file = new TestJsonFile("audit-exposure-timestamp");
        await ReplaceSegmentRecordsAsync(file.Path, [document]);
        using var sink = new LocalFileAuditSink(
                new LocalFileAuditSinkOptions(file.Path));

        Assert.False(await sink.IsAvailableAsync(CancellationToken.None));
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
        var service = new ExposureConfirmationService(
            store,
            sink,
            new BoundedPostAuditExposureCommitPolicy(
                TimeSpan.FromSeconds(5),
                TimeProvider.System,
                CancellationToken.None,
                Microsoft.Extensions.Logging.Abstractions.NullLogger<
                    BoundedPostAuditExposureCommitPolicy>.Instance));
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
            Assert.Single(await ReadAuditLinesAsync(auditPath));
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
        Evidence: new DecisionEvidenceSnapshot(
            0.82,
            0.2,
            0.74,
            50,
            new Dictionary<string, JsonElement>
            {
                ["source"] = JsonSerializer.SerializeToElement(
                    "phase3-local-fixture")
            }),
        Confidence: new ConfidenceReport(0.82, 0.2, 0.74),
        StrategyId: "strategy-tetris-balanced-v1");

    private static ExposureAuditRecord ExposureRecord(string exposureId) =>
        new(
            exposureId,
            $"decision-{exposureId}",
            "tetris-demo",
            "dev",
            null,
            "2026-08-06T00:00:02Z");

    private static ExposureAuditRecord CanonicalExposureRecord() =>
        new(
            "exposure-1",
            "decision-1",
            "tetris-demo",
            "dev",
            "2026-08-06T00:00:01Z",
            "2026-08-06T00:00:02Z");

    private static string DecisionEnvelope(Action<JsonObject> mutate)
    {
        var envelope = JsonSerializer.SerializeToNode(
            new { kind = "decision", record = DecisionRecord() },
            new JsonSerializerOptions(JsonSerializerDefaults.Web))!.AsObject();
        mutate(envelope["record"]!.AsObject());
        return envelope.ToJsonString();
    }

    private static string ExposureEnvelope(Action<JsonObject> mutate)
    {
        var envelope = JsonSerializer.SerializeToNode(
            new
            {
                kind = "exposure",
                record = new ExposureAuditRecord(
                    "exposure-1",
                    "decision-1",
                    "tetris-demo",
                    "dev",
                    "2026-08-06T00:00:01Z",
                    "2026-08-06T00:00:02Z")
            },
            new JsonSerializerOptions(JsonSerializerDefaults.Web))!.AsObject();
        mutate(envelope["record"]!.AsObject());
        return envelope.ToJsonString();
    }

    private static JsonElement JsonValue(string rawValue)
    {
        using var document = JsonDocument.Parse(rawValue);
        return document.RootElement.Clone();
    }

    private static IEnumerable<(string Name, string Value)>
        StrictStringContaminations()
    {
        yield return ("space", " ");
        yield return ("tab", "\t");
        yield return ("line feed", "\n");
        yield return ("carriage return", "\r");
    }

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

    private static async Task<string[]> ReadAuditLinesAsync(string path)
    {
        var result = new List<string>();
        foreach (var segment in SegmentPaths(path))
        {
            result.AddRange((await File.ReadAllLinesAsync(segment)).Skip(1));
        }
        return result.ToArray();
    }

    private static async Task ReplaceSegmentRecordsAsync(
        string path,
        IReadOnlyList<string> records,
        string separator = "\n",
        bool terminate = true)
    {
        using (var initializer = new LocalFileAuditSink(
                   new LocalFileAuditSinkOptions(path)))
        {
            await initializer.RecordDecisionAsync(
                DecisionRecord(),
                CancellationToken.None);
        }

        var manifestPath = System.IO.Path.Combine($"{path}.d", "manifest.json");
        var manifest = JsonNode.Parse(await File.ReadAllTextAsync(manifestPath))!
            .AsObject();
        var segment = manifest["segments"]!.AsArray()
            .Select(item => item!.AsObject())
            .Single(item => item["current"]!.GetValue<bool>());
        var segmentPath = System.IO.Path.Combine(
            $"{path}.d",
            "segments",
            segment["fileName"]!.GetValue<string>());
        var header = (await File.ReadAllLinesAsync(segmentPath))[0];
        var content = new StringBuilder(header).Append(separator);
        for (var index = 0; index < records.Count; index++)
        {
            content.Append(records[index]);
            if (terminate || index < records.Count - 1)
            {
                content.Append(separator);
            }
        }
        var bytes = Encoding.UTF8.GetBytes(content.ToString());
        await File.WriteAllBytesAsync(segmentPath, bytes);
        segment["length"] = bytes.LongLength;
        segment["recordCount"] = records.Count;
        segment["contentHash"] =
            $"sha256:{Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant()}";
        manifest["exposures"] = new JsonArray();
        await File.WriteAllTextAsync(
            manifestPath,
            $"{manifest.ToJsonString()}\n");
    }

    private static async Task<JsonObject> ReadManifestAsync(string path) =>
        JsonNode.Parse(
            await File.ReadAllTextAsync(
                System.IO.Path.Combine($"{path}.d", "manifest.json")))!
            .AsObject();

    private static async Task<string> CurrentSegmentPathAsync(string path)
    {
        var manifest = await ReadManifestAsync(path);
        var segment = manifest["segments"]!.AsArray()
            .Select(item => item!.AsObject())
            .Single(item => item["current"]!.GetValue<bool>());
        return System.IO.Path.Combine(
            $"{path}.d",
            "segments",
            segment["fileName"]!.GetValue<string>());
    }

    private static string[] SegmentPaths(string path)
    {
        var directory = System.IO.Path.Combine($"{path}.d", "segments");
        if (!Directory.Exists(directory))
        {
            return [];
        }
        var closed = Directory.GetFiles(directory, "segment-*.jsonl")
            .Order(StringComparer.Ordinal)
            .ToList();
        var current = System.IO.Path.Combine(directory, "current.jsonl");
        if (File.Exists(current))
        {
            closed.Add(current);
        }
        return closed.ToArray();
    }

    private static string[] MarkerPaths(string path)
    {
        var directory = System.IO.Path.Combine($"{path}.d", "exposures");
        return Directory.Exists(directory)
            ? Directory.GetFiles(directory, "*.json")
            : [];
    }

    private sealed class SimulatedCrashException : Exception;

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

        var auditDirectory = $"{Path}.d";
        if (Directory.Exists(auditDirectory))
        {
            Directory.Delete(auditDirectory, recursive: true);
        }

        var parent = System.IO.Path.GetDirectoryName(Path);
        var name = System.IO.Path.GetFileName(Path);
        if (parent is not null && Directory.Exists(parent))
        {
            foreach (var stagingDirectory in Directory.GetDirectories(
                         parent,
                         $"{name}.d.tmp-*"))
            {
                Directory.Delete(stagingDirectory, recursive: true);
            }
        }

    }
}

internal sealed class RecordingAuditDirectoryOperations :
    IDurableDirectoryOperations
{
    private readonly object _gate = new();
    private bool _created;
    private bool _failed;

    public List<string> Events { get; } = [];

    public bool FailAfterFirstCreate { get; init; }

    public bool Exists(string path) => Directory.Exists(path);

    public void Create(string path)
    {
        lock (_gate)
        {
            Events.Add($"mkdir:{path}");
        }
        Directory.CreateDirectory(path);
        _created = true;
    }

    public void Flush(string path)
    {
        lock (_gate)
        {
            Events.Add($"sync:{path}");
        }
        if (FailAfterFirstCreate && _created && !_failed)
        {
            _failed = true;
            throw new IOException("Injected durable parent sync failure.");
        }
    }
}
