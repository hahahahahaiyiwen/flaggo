using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Flaggo.Evidence;
using Flaggo.Registry;
using Flaggo.Shared.Contracts;
using Flaggo.State;
using static Flaggo.Decisioning.Tests.InputEvidenceTestData;

namespace Flaggo.Decisioning.Tests;

public sealed class InputEvidenceMaterializerTests
{
    [Theory]
    [InlineData("gauge", "0.25")]
    [InlineData("span-duration", "125.25")]
    [InlineData("span-attribute", "true")]
    [InlineData("span-event", "\"placed\"")]
    [InlineData("log-attribute", "0.25")]
    [InlineData("log-body", "0.25")]
    public async Task NativeProjection_PreservesPrimitiveTimestampAndObservedProvenance(string kind, string expected)
    {
        var binding = Binding();
        var observation = Observation();
        switch (kind)
        {
            case "span-duration":
                binding = binding with
                {
                    Unit = "ms", Maximum = 5000,
                    Source = binding.Source with { Kind = "span", ValueFrom = "duration" }
                };
                observation = observation with { Kind = "span", StartTimeUnixNano = Timestamp - 125_250_000 };
                break;
            case "span-attribute":
                binding = binding with
                {
                    ValueType = "boolean", Unit = null, Minimum = null, Maximum = null,
                    Source = binding.Source with { Kind = "span", ValueFrom = "attributes", ValueKey = "enabled" }
                };
                observation = observation with { Kind = "span", Attributes = Map(("enabled", true)) };
                break;
            case "span-event":
                binding = binding with
                {
                    ValueType = "string", Unit = null, Minimum = null, Maximum = null,
                    Source = binding.Source with
                    {
                        Kind = "span", EventName = "work.completed", ValueFrom = "eventAttributes", ValueKey = "result"
                    }
                };
                observation = observation with
                {
                    Kind = "span",
                    Events = [new("ignored", Timestamp - 200, Map(("result", "wrong"))),
                        new("work.completed", Timestamp - 100, Map(("result", "placed")))]
                };
                break;
            case "log-attribute":
                binding = binding with { Source = binding.Source with
                    { Kind = "log", Name = null, EventName = "work.completed", ValueFrom = "attributes", ValueKey = "value" } };
                observation = observation with { Kind = "log", EventName = "work.completed", Attributes = Map(("value", 0.25)) };
                break;
            case "log-body":
                binding = binding with { Source = binding.Source with
                    { Kind = "log", Name = null, EventName = "work.completed", ValueFrom = "body", BodyPath = ["work", "value"] } };
                observation = observation with
                {
                    Kind = "log", EventName = "work.completed",
                    Body = JsonSerializer.SerializeToElement(new { work = new { value = 0.25 } })
                };
                break;
        }
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        var result = await materializer.IngestAsync(Scope, [observation], CancellationToken.None);
        Assert.Equal(1, result.AcceptedRecords);
        Assert.Equal(0, result.RejectedRecords);
        var value = await ReadAsync(materializer, binding);
        Assert.Equal("available", value.Status);
        using var expectedValue = JsonDocument.Parse(expected);
        Assert.True(JsonElement.DeepEquals(expectedValue.RootElement, value.Value!.Value));
        Assert.Equal((kind == "span-event" ? Timestamp - 100 : Timestamp).ToString(), value.Provenance.ObservedTimeUnixNano);
        Assert.Equal("observed", value.Provenance.Coverage);
        Assert.Equal("evidence", value.Provenance.Source);
        Assert.Equal(1u, value.Provenance.SamplingFlags);
        Assert.Equal(ObservedAt, value.Provenance.MaterializedAt);
        Assert.NotNull(value.Provenance.SourceFingerprint);
    }

    [Theory]
    [InlineData("scope")]
    [InlineData("scope-version")]
    [InlineData("resource")]
    [InlineData("record-namespace")]
    [InlineData("name")]
    [InlineData("foreign-application")]
    [InlineData("foreign-environment")]
    public async Task SelectorsAndAuthorizedScope_DoNotMixUnrelatedObservations(string mismatch)
    {
        var binding = Binding() with { Source = Binding().Source with
            { ScopeVersion = "1", ResourceAttributes = Map(("service.name", "worker")) } };
        var observation = Observation() with { ResourceAttributes = Map(("service.name", "worker")) };
        var scope = Scope;
        switch (mismatch)
        {
            case "scope": observation = observation with { ScopeName = "other" }; break;
            case "scope-version": observation = observation with { ScopeVersion = "2" }; break;
            case "resource": observation = observation with { ResourceAttributes = Map(("service.name", "other")) }; break;
            case "record-namespace": observation = observation with
                { ResourceAttributes = Map(), Attributes = Map(("service.name", "worker")) }; break;
            case "name": observation = observation with { Name = "other" }; break;
            case "foreign-application": scope = new("foreign", Scope.Environment, Scope.TenantId); break;
            case "foreign-environment": scope = new(Scope.AppId, "production", Scope.TenantId); break;
        }
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        var result = await materializer.IngestAsync(scope, [observation], CancellationToken.None);
        Assert.Equal(1, result.UnmatchedRecords);
        Assert.Equal("missing", (await ReadAsync(materializer, binding)).Status);
    }

    [Theory]
    [InlineData(-1, "future")]
    [InlineData(0, "available")]
    [InlineData(299999999, "available")]
    [InlineData(300000000, "available")]
    [InlineData(300000001, "stale")]
    public async Task Freshness_UsesSourceTimeWithInclusiveMaximum(long elapsedTicks, string status)
    {
        var binding = Binding();
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation(0)], CancellationToken.None);
        var result = await ReadAsync(materializer, binding, ObservedAt.AddTicks(elapsedTicks));
        Assert.Equal(status, result.Status);
        if (status == "available") Assert.Equal(0, result.Value!.Value.GetDouble());
        else Assert.Null(result.Value);
    }

    [Fact]
    public async Task SameApplicationAndDefinition_DoNotShareFramesAcrossTenants()
    {
        var binding = Binding();
        var foreign = Scope with { TenantId = "tenant-b" };
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation(0.25)], CancellationToken.None);
        var request = new InputEvidenceRequest(foreign, Identity, [new("pressure", binding, Global)], ObservedAt);
        Assert.Equal("missing", (await materializer.ReadInputsAsync(request, CancellationToken.None)).Inputs["pressure"].Status);

        await materializer.IngestAsync(foreign, [Observation(0.75)], CancellationToken.None);
        Assert.Equal(0.25, (await ReadAsync(materializer, binding)).Value!.Value.GetDouble());
        Assert.Equal(0.75, (await materializer.ReadInputsAsync(request, CancellationToken.None)).Inputs["pressure"].Value!.Value.GetDouble());
    }

    [Theory]
    [InlineData("missing", "invalid-value")]
    [InlineData("wrong-type", "invalid-value")]
    [InlineData("out-of-range", "invalid-value")]
    [InlineData("unit", "unit-mismatch")]
    [InlineData("no-recorded-value", "no-recorded-value")]
    [InlineData("sum", "unsupported-metric-type")]
    [InlineData("timestamp", "missing")]
    [InlineData("target", "missing")]
    public async Task InvalidObservation_IsExplicitlyRejectedNeverConvertedToZero(string mutation, string status)
    {
        var binding = Binding();
        var observation = Observation();
        switch (mutation)
        {
            case "missing": observation = observation with { Value = null }; break;
            case "wrong-type": observation = observation with { Value = JsonSerializer.SerializeToElement("0.25") }; break;
            case "out-of-range": observation = observation with { Value = JsonSerializer.SerializeToElement(2) }; break;
            case "unit": observation = observation with { Unit = "seconds" }; break;
            case "no-recorded-value": observation = observation with { NoRecordedValue = true }; break;
            case "sum": observation = observation with { DataType = "sum" }; break;
            case "timestamp": observation = observation with { TimeUnixNano = 0 }; break;
            case "target": binding = binding with
                { TargetType = "session", TargetIdAttribute = new("attributes", "session.id") }; break;
        }
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        var result = await materializer.IngestAsync(Scope, [observation], CancellationToken.None);
        Assert.Equal(1, result.RejectedRecords);
        Assert.NotEmpty(result.Diagnostics);
        var value = await ReadAsync(materializer, binding);
        Assert.Equal(status, value.Status);
        Assert.Null(value.Value);
    }

    [Fact]
    public async Task RetryAndOutOfOrderDelivery_DoNotRefreshOrReplaceLatestValue()
    {
        var binding = Binding();
        var store = new MemoryEvidenceStore();
        var clock = new EvidenceClock();
        await using var materializer = Materializer(binding, store, clock);
        await materializer.InitializeAsync();
        var observation = Observation();
        await materializer.IngestAsync(Scope, [observation], CancellationToken.None);
        var first = await ReadAsync(materializer, binding);
        clock.Now += TimeSpan.FromSeconds(20);
        await materializer.IngestAsync(Scope, [observation, Observation(0.9, Timestamp - 100)], CancellationToken.None);
        var replay = await ReadAsync(materializer, binding, clock.Now);
        Assert.Equal(first, replay);
        Assert.Equal(1, store.Publications);
        Assert.Equal("stale", (await ReadAsync(materializer, binding, ObservedAt.AddSeconds(31))).Status);
    }

    [Fact]
    public async Task SameTimeConflictAndMultipleFreshGaugeStreams_AreAmbiguous()
    {
        var binding = Binding();
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation(), Observation(0.9)], CancellationToken.None);
        Assert.Equal("ambiguous", (await ReadAsync(materializer, binding)).Status);
        await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
        Assert.Equal("ambiguous", (await ReadAsync(materializer, binding)).Status);

        await using var streams = Materializer(binding);
        await streams.InitializeAsync();
        await streams.IngestAsync(Scope,
            [Observation() with { MetricStreamFingerprint = $"sha256:{new string('b', 64)}" },
             Observation(0.9) with { MetricStreamFingerprint = $"sha256:{new string('c', 64)}" }], CancellationToken.None);
        Assert.Equal("ambiguous", (await ReadAsync(streams, binding)).Status);
    }

    [Fact]
    public async Task NewerInvalidObservation_InvalidatesLastGoodValue()
    {
        var binding = Binding();
        await using var materializer = Materializer(binding);
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
        await materializer.IngestAsync(Scope, [Observation(0, Timestamp + 100) with { NoRecordedValue = true }], CancellationToken.None);
        Assert.Equal("no-recorded-value", (await ReadAsync(materializer, binding, ObservedAt.AddTicks(1))).Status);
    }

    [Fact]
    public async Task RequiredInputsReadOneAtomicGeneration_AndUncertainPublicationFailsClosed()
    {
        var binding = Binding();
        var second = binding with { Key = "second" };
        var store = new MemoryEvidenceStore();
        await using var materializer = Materializer(binding, store, additional: [second]);
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
        var before = store.Snapshot.Generation;
        store.PublicationGate = new(TaskCreationOptions.RunContinuationsAsynchronously);
        store.PublicationStarted = new(TaskCreationOptions.RunContinuationsAsynchronously);
        var ingestion = materializer.IngestAsync(Scope, [Observation(0.8, Timestamp + 100)], CancellationToken.None);
        await store.PublicationStarted.Task.WaitAsync(TimeSpan.FromSeconds(5));
        var request = new InputEvidenceRequest(Scope, Identity,
            [new("a", binding, Global), new("b", second, Global)], ObservedAt.AddTicks(1));
        var pinned = await materializer.ReadInputsAsync(request, CancellationToken.None);
        Assert.Equal(before, pinned.Generation);
        Assert.All(pinned.Inputs.Values, input => Assert.Equal(0.25, input.Value!.Value.GetDouble()));
        store.PublicationGate.SetResult();
        await ingestion;
        var after = await materializer.ReadInputsAsync(request, CancellationToken.None);
        Assert.NotEqual(before, after.Generation);
        Assert.All(after.Inputs.Values, input =>
        {
            Assert.Equal(0.8, input.Value!.Value.GetDouble());
            Assert.Equal(after.Generation, input.Provenance.Generation);
        });

        store.Failure = new IOException("Publication completion is uncertain.");
        await Assert.ThrowsAsync<InputEvidenceUnavailableException>(() =>
            materializer.IngestAsync(Scope, [Observation(0.9, Timestamp + 200)], CancellationToken.None));
        Assert.False(materializer.IsInitialized);
        await Assert.ThrowsAsync<InputEvidenceUnavailableException>(() => materializer.ReadInputsAsync(request, CancellationToken.None));
        store.Failure = null;
        await materializer.InitializeAsync();
        Assert.Equal(after.Generation, (await materializer.ReadInputsAsync(request, CancellationToken.None)).Generation);
    }

    [Fact]
    public async Task CapacityFailure_DoesNotEvictLiveValuesOrAcknowledgeUncommittedData()
    {
        var binding = Binding();
        var store = new MemoryEvidenceStore();
        await using var materializer = Materializer(binding, store, limits: new(MaximumFrames: 1));
        await materializer.InitializeAsync();
        await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
        await Assert.ThrowsAsync<InputEvidenceCapacityException>(() => materializer.IngestAsync(
            Scope, [Observation(0.8) with { MetricStreamFingerprint = $"sha256:{new string('c', 64)}" }], CancellationToken.None));
        Assert.Equal(0.25, (await ReadAsync(materializer, binding)).Value!.Value.GetDouble());
        Assert.Equal(1, store.Publications);
    }

    [Fact]
    public async Task PartialSuccess_CountsARecordOnlyWhenAllMatchingBindingsRejectIt()
    {
        var binding = Binding();
        var strict = binding with { Key = "strict", Maximum = 0.1 };
        await using var materializer = Materializer(binding, additional: [strict]);
        await materializer.InitializeAsync();

        var partial = await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
        Assert.Equal(1, partial.AcceptedRecords);
        Assert.Equal(0, partial.RejectedRecords);
        Assert.Contains("strict: invalid-value", partial.Diagnostics);
        Assert.Equal("available", (await ReadAsync(materializer, binding)).Status);
        Assert.Equal("invalid-value", (await ReadAsync(materializer, strict)).Status);

        var rejected = await materializer.IngestAsync(Scope, [Observation(2, Timestamp + 100)], CancellationToken.None);
        Assert.Equal(0, rejected.AcceptedRecords);
        Assert.Equal(1, rejected.RejectedRecords);
    }

    [Fact]
    public async Task DurableStore_RestartRetainsSourceTimeAndRejectsCorruptionAndCompetingWriter()
    {
        using var directory = new TestRegistryFile();
        var path = Path.Combine(Path.GetDirectoryName(directory.Path)!, "telemetry", "current.commit.json");
        var binding = Binding();
        string generation;
        await using (var materializer = Materializer(binding, new LocalInputEvidenceStore(new(path))))
        {
            await materializer.InitializeAsync();
            await materializer.IngestAsync(Scope, [Observation()], CancellationToken.None);
            generation = (await ReadAsync(materializer, binding)).Provenance.Generation!;
            await using var competitor = new LocalInputEvidenceStore(new(path));
            await Assert.ThrowsAsync<IOException>(() => competitor.LoadAsync(CancellationToken.None));
        }
        await using (var restarted = Materializer(binding, new LocalInputEvidenceStore(new(path))))
        {
            await restarted.InitializeAsync();
            var value = await ReadAsync(restarted, binding);
            Assert.Equal(generation, value.Provenance.Generation);
            Assert.Equal(Timestamp.ToString(), value.Provenance.ObservedTimeUnixNano);
        }
        var descriptor = await CommittedFileSnapshot.ResolveAsync(
            CommittedFileSnapshotSource.FromDescriptor(path), null, CancellationToken.None);
        await File.WriteAllTextAsync(descriptor.ArtifactPath, "{}");
        await using var corrupt = Materializer(binding, new LocalInputEvidenceStore(new(path)));
        await Assert.ThrowsAsync<InputEvidenceUnavailableException>(() => corrupt.InitializeAsync());
        Assert.Equal("{}", await File.ReadAllTextAsync(descriptor.ArtifactPath));
    }

    [Fact]
    public async Task Attribution_RequiresCommittedExposureWithExactScopeDefinitionAndTarget()
    {
        var binding = Binding() with
        {
            ExposureIdAttribute = new("attributes", "flaggo.exposure.id"),
            Source = Binding().Source with { Kind = "span", ValueFrom = "attributes", ValueKey = "value" }
        };
        var exposures = new InMemoryExposureStore(new EvidenceClock(), () => "confirmed");
        var snapshot = new DecisionSnapshot(Scope.TenantId, Scope.AppId, Scope.Environment, Identity,
            JsonSerializer.SerializeToElement(0.25), "number", new("server", false, false, null),
            Map(), Map(), Global, Global, [], ["global"], new("approved", [], []),
            RequestInputs: Map(), InputProvenance: new Dictionary<string, InputProvenance>(), ResolvedTargets: [Global]);
        await exposures.CreatePendingAsync("decision", "token", snapshot, CancellationToken.None);
        var preparation = await exposures.PrepareConfirmationAsync(Scope.TenantId, "decision", new("token"),
            new HashSet<string> { Scope.AppId }, new HashSet<string> { Scope.Environment }, CancellationToken.None);
        Assert.Null(await exposures.FindConfirmedAsync("confirmed", Scope, CancellationToken.None));
        await using var materializer = Materializer(binding, exposures: exposures);
        await materializer.InitializeAsync();
        var observation = Observation() with
        {
            Kind = "span", Attributes = Map(("value", 0.25), ("flaggo.exposure.id", "confirmed"))
        };
        Assert.Equal(1, (await materializer.IngestAsync(Scope, [observation], CancellationToken.None)).RejectedRecords);
        await exposures.CommitConfirmationAsync("decision", preparation.Outcome.Result.ExposureId, CancellationToken.None);
        Assert.Null(await exposures.FindConfirmedAsync("confirmed", new("foreign", "dev", Scope.TenantId), CancellationToken.None));
        Assert.Null(await exposures.FindConfirmedAsync("confirmed", Scope with { TenantId = "tenant-b" }, CancellationToken.None));
        Assert.Equal(1, (await materializer.IngestAsync(Scope,
            [observation with { TimeUnixNano = Timestamp + 100 }], CancellationToken.None)).AcceptedRecords);
        Assert.Equal("confirmed", (await ReadAsync(materializer, binding, ObservedAt.AddTicks(1))).Provenance.ExposureId);

        await using var foreign = Materializer(binding, exposures: exposures, identity: Identity with { Revision = "foreign" });
        await foreign.InitializeAsync();
        Assert.Equal(1, (await foreign.IngestAsync(Scope, [observation], CancellationToken.None)).RejectedRecords);
        var targetBinding = binding with { TargetType = "session", TargetIdAttribute = new("attributes", "session.id") };
        await using var otherTarget = Materializer(targetBinding, exposures: exposures);
        await otherTarget.InitializeAsync();
        Assert.Equal(1, (await otherTarget.IngestAsync(Scope,
            [observation with { Attributes = Map(("value", 0.25), ("flaggo.exposure.id", "confirmed"), ("session.id", "other")) }],
            CancellationToken.None)).RejectedRecords);
    }

    [Fact]
    public async Task RegistryCorruption_IsAvailabilityFailureRatherThanMalformedTelemetry()
    {
        await using var materializer = new InputEvidenceMaterializer(new BrokenBindings(),
            new InMemoryExposureStore(new EvidenceClock(), () => "exposure"), new MemoryEvidenceStore(), new EvidenceClock());
        await materializer.InitializeAsync();
        var error = await Assert.ThrowsAsync<InputEvidenceUnavailableException>(() =>
            materializer.IngestAsync(Scope, [Observation()], CancellationToken.None));
        Assert.IsType<JsonException>(error.InnerException);
    }

    private sealed class BrokenBindings : IEvidenceBindingReader
    {
        public Task<IReadOnlyList<EvidenceBindingProjection>> ReadBindingsAsync(ApplicationScope scope, CancellationToken cancellationToken) =>
            Task.FromException<IReadOnlyList<EvidenceBindingProjection>>(new JsonException("Corrupt registry."));
    }
}

internal static class InputEvidenceTestData
{
    public static readonly ApplicationScope Scope = new("worker", "dev", "tenant-a");
    public static readonly RuntimeContractIdentity Identity = new("definition", $"sha256:{new string('a', 64)}", "revision");
    public static readonly DecisionTargetRef Global = new("global", "global");
    public static readonly DateTimeOffset ObservedAt = new(2026, 9, 1, 0, 0, 0, TimeSpan.Zero);
    public static readonly ulong Timestamp = checked((ulong)(ObservedAt.UtcTicks - DateTimeOffset.UnixEpoch.UtcTicks) * 100);

    public static IReadOnlyDictionary<string, JsonElement> Map(params (string Key, object Value)[] values) =>
        values.ToDictionary(pair => pair.Key, pair => JsonSerializer.SerializeToElement(pair.Value), StringComparer.Ordinal);

    public static RegisteredEvidenceBinding Binding() => new("pressure", "Latest received pressure.", "number", "1", 0, 1,
        new("metric", "worker", null, "work.pressure", null, null, Map(), Map(), Map(), "value", null, []),
        "global", null, 30, null);

    public static TelemetryObservation Observation(double value = 0.25, ulong? timestamp = null) =>
        new("metric", "work.pressure", "worker", "1", Map(), Map(), timestamp ?? Timestamp,
            $"sha256:{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes($"{value:R}:{timestamp ?? Timestamp}"))).ToLowerInvariant()}",
            MetricStreamFingerprint: $"sha256:{new string('b', 64)}", DataType: "gauge", Unit: "1",
            Value: JsonSerializer.SerializeToElement(value), SamplingFlags: 1);

    public static RuntimeDecisionDefinition Definition(RegisteredEvidenceBinding binding, RuntimeContractIdentity? identity = null,
        IReadOnlyList<RegisteredEvidenceBinding>? additional = null) =>
        new(Scope.AppId, Scope.Environment, "worker.limit", identity ?? Identity, "number",
            JsonSerializer.SerializeToElement(800), "safe-default",
            [new("pressure", binding.ValueType, "evidence", binding.Meaning, binding.Minimum, binding.Maximum, binding.Unit, binding.Key)],
            [], NumberActionSpace: new(0, 1500), Policy: new(),
            TargetHierarchy: ["session", "global"], InferenceTarget: "global", FallbackOrder: [],
            Evidence: new[] { binding }.Concat(additional ?? []).ToArray());

    public static InputEvidenceMaterializer Materializer(RegisteredEvidenceBinding binding,
        IInputEvidenceSnapshotStore? store = null, EvidenceClock? clock = null, InputEvidenceLimits? limits = null,
        IConfirmedExposureReader? exposures = null, RuntimeContractIdentity? identity = null,
        IReadOnlyList<RegisteredEvidenceBinding>? additional = null) =>
        new(new InMemoryDefinitionRegistry([Definition(binding, identity, additional)]),
            exposures ?? new InMemoryExposureStore(clock ?? new EvidenceClock(), () => "exposure"),
            store ?? new MemoryEvidenceStore(), clock ?? new EvidenceClock(), limits);

    public static async Task<InputEvidenceValue> ReadAsync(IInputEvidenceReader reader, RegisteredEvidenceBinding binding,
        DateTimeOffset? now = null) =>
        (await reader.ReadInputsAsync(new(Scope, Identity,
            [new("pressure", binding, binding.TargetType == "global" ? Global : new("session", "session-1"))],
            now ?? ObservedAt), CancellationToken.None)).Inputs["pressure"];
}

internal sealed class EvidenceClock : TimeProvider
{
    public DateTimeOffset Now { get; set; } = InputEvidenceTestData.ObservedAt;
    public override DateTimeOffset GetUtcNow() => Now;
}

internal sealed class MemoryEvidenceStore : IInputEvidenceSnapshotStore
{
    public InputEvidenceSnapshot Snapshot { get; private set; } = new(1, Guid.NewGuid().ToString("N"), []);
    public int Publications { get; private set; }
    public Exception? Failure { get; set; }
    public TaskCompletionSource? PublicationStarted { get; set; }
    public TaskCompletionSource? PublicationGate { get; set; }
    public Task<InputEvidenceSnapshot> LoadAsync(CancellationToken cancellationToken) => Task.FromResult(Snapshot);
    public async Task PublishAsync(InputEvidenceSnapshot snapshot, CancellationToken cancellationToken)
    {
        PublicationStarted?.TrySetResult();
        if (PublicationGate is not null) await PublicationGate.Task.WaitAsync(cancellationToken);
        if (Failure is not null) throw Failure;
        Snapshot = snapshot;
        Publications++;
    }
    public ValueTask DisposeAsync() => ValueTask.CompletedTask;
}
