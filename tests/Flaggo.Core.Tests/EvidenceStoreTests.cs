using System.Text.Json;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Microsoft.Data.Sqlite;

namespace Flaggo.Core.Tests;

public sealed class EvidenceStoreTests
{
    private static readonly DecisionScope Scope = new("checkout", "production");

    [Fact]
    public async Task TelemetryRecordsAreImmutableScopedAndPersistent()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteEvidenceStore(database.ConnectionString);
        await store.InitializeAsync();
        var observedAt = new DateTimeOffset(2026, 4, 1, 10, 0, 0, TimeSpan.Zero);
        var record = TelemetryRecord(
            observationId: "decision-1",
            signal: "decision.received",
            observedAt: observedAt,
            result: 850);

        Assert.Equal(
            new EvidenceTelemetryWriteResult(Created: 1, Existing: 0),
            await store.PutTelemetryBatchAsync([record]));
        Assert.Equal(
            new EvidenceTelemetryWriteResult(Created: 0, Existing: 1),
            await store.PutTelemetryBatchAsync([
                record with
                {
                    Payload = Payload("decision.received", 900),
                    ObservedAt = observedAt.AddMinutes(1)
                }
            ]));

        var persisted = await new SqliteEvidenceStore(database.ConnectionString)
            .ListTelemetryAsync(Scope, limit: 10, signal: "decision.received");

        var actual = Assert.Single(persisted);
        Assert.Equal("decision-1", actual.ObservationId);
        Assert.Equal("decision.received", actual.Signal);
        Assert.Equal(observedAt, actual.ObservedAt);
        Assert.Equal("flaggo.decision.received", actual.Payload.GetProperty("eventName").GetString());
        Assert.Equal("850", actual.Payload
            .GetProperty("attributes")[2]
            .GetProperty("value")
            .GetProperty("stringValue")
            .GetString());
        Assert.Empty(await store.ListTelemetryAsync(
            new DecisionScope("checkout", "staging"),
            limit: 10));
    }

    [Fact]
    public async Task TelemetryBatchPersistsMultipleSignalsAndCanFilter()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteEvidenceStore(database.ConnectionString);
        await store.InitializeAsync();

        var write = await store.PutTelemetryBatchAsync([
            TelemetryRecord("decision-1", "decision.received"),
            TelemetryRecord("outcome-1", "outcome.observed")
        ]);

        Assert.Equal(new EvidenceTelemetryWriteResult(Created: 2, Existing: 0), write);
        Assert.Equal(2, (await store.ListTelemetryAsync(Scope, 10)).Count);
        var outcomes = await store.ListTelemetryAsync(Scope, 10, "outcome.observed");
        var outcome = Assert.Single(outcomes);
        Assert.Equal("outcome-1", outcome.ObservationId);
    }

    [Fact]
    public async Task EvidenceStoreSharesDatabaseAndReportsAvailability()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteEvidenceStore(database.ConnectionString);
        await store.InitializeAsync();

        Assert.True(await store.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "UPDATE flaggo_schema_versions SET version = 2 "
            + "WHERE component = 'evidence-store';");
        Assert.False(await store.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "UPDATE flaggo_schema_versions SET version = 1 "
            + "WHERE component = 'evidence-store'; "
            + "DROP TABLE evidence_telemetry_records;");
        Assert.False(await store.IsAvailableAsync());
    }

    private static EvidenceTelemetryRecord TelemetryRecord(
        string observationId,
        string signal,
        DateTimeOffset? observedAt = null,
        int result = 850) =>
        new(
            Scope,
            observationId,
            signal,
            Payload(signal, result),
            observedAt ?? new DateTimeOffset(2026, 4, 1, 10, 0, 0, TimeSpan.Zero),
            new DateTimeOffset(2026, 4, 1, 10, 0, 1, TimeSpan.Zero));

    private static JsonElement Payload(string signal, int result)
    {
        var eventName = signal == "decision.received"
            ? "flaggo.decision.received"
            : "flaggo.outcome.observed";
        return JsonSerializer.SerializeToElement(new
        {
            eventName,
            timeUnixNano = "1770000000000000000",
            attributes = new object[]
            {
                Attribute("flaggo.signal", signal),
                Attribute("flaggo.decision.id", "decision-1"),
                Attribute("flaggo.result.json", result.ToString())
            }
        });
    }

    private static object Attribute(string key, string value) => new
    {
        key,
        value = new { stringValue = value }
    };

    private static async Task ExecuteSqlAsync(
        string connectionString,
        string commandText)
    {
        await using var connection = new SqliteConnection(connectionString);
        await connection.OpenAsync();
        await using var command = connection.CreateCommand();
        command.CommandText = commandText;
        await command.ExecuteNonQueryAsync();
    }

    private sealed class TemporaryDatabase : IDisposable
    {
        private readonly string _path =
            Path.Combine(Path.GetTempPath(), $"flaggo-evidence-{Guid.NewGuid():N}.db");

        public string ConnectionString => $"Data Source={_path};Pooling=False";

        public void Dispose()
        {
            DeleteIfExists(_path);
            DeleteIfExists($"{_path}-shm");
            DeleteIfExists($"{_path}-wal");
        }

        private static void DeleteIfExists(string path)
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
    }
}
