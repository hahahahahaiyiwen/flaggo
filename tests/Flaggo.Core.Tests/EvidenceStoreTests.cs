using System.Text.Json;
using Flaggo.Contract;
using Flaggo.EvidenceStore;
using Microsoft.Data.Sqlite;

namespace Flaggo.Core.Tests;

public sealed class EvidenceStoreTests
{
    private static readonly DecisionScope Scope = new("checkout", "production");
    private const string ContractDigest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    private const string ExecutableDigest =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    private const string ResultHash =
        "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    [Fact]
    public async Task DecisionObservationsAreImmutableScopedAndPersistent()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteEvidenceStore(database.ConnectionString);
        await store.InitializeAsync();
        var observedAt = new DateTimeOffset(2026, 4, 1, 10, 0, 0, TimeSpan.Zero);
        var observation = DecisionObservation(
            decisionId: "decision-1",
            observedAt: observedAt,
            result: 850);

        Assert.Equal(
            EvidenceObservationWriteResult.Created,
            await store.PutDecisionAsync(observation));
        Assert.Equal(
            EvidenceObservationWriteResult.Existing,
            await store.PutDecisionAsync(observation with
            {
                Result = JsonSerializer.SerializeToElement(900),
                ObservedAt = observedAt.AddMinutes(1)
            }));

        var persisted = await new SqliteEvidenceStore(database.ConnectionString)
            .ListDecisionsAsync(Scope, limit: 10, contractName: "tetris.dropInterval");

        var actual = Assert.Single(persisted);
        Assert.Equal("decision-1", actual.DecisionId);
        Assert.Equal(850, actual.Result.GetInt32());
        Assert.Equal(observedAt, actual.ObservedAt);
        Assert.Equal("session-1", actual.CorrelationAttributes["sessionId"].GetString());
        Assert.Empty(await store.ListDecisionsAsync(
            new DecisionScope("checkout", "staging"),
            limit: 10));
    }

    [Fact]
    public async Task OutcomeObservationsAreImmutableScopedAndQueryableByBinding()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteEvidenceStore(database.ConnectionString);
        await store.InitializeAsync();
        var observedAt = new DateTimeOffset(2026, 4, 1, 10, 5, 0, TimeSpan.Zero);
        var observation = OutcomeObservation(
            observationId: "outcome-1",
            binding: "tetris.survival_ms",
            observedAt: observedAt,
            value: 18_400);

        Assert.Equal(
            EvidenceObservationWriteResult.Created,
            await store.PutOutcomeAsync(observation));
        Assert.Equal(
            EvidenceObservationWriteResult.Existing,
            await store.PutOutcomeAsync(observation with
            {
                Value = JsonSerializer.SerializeToElement(1),
                ObservedAt = observedAt.AddMinutes(1)
            }));

        var persisted = await new SqliteEvidenceStore(database.ConnectionString)
            .ListOutcomesAsync(Scope, limit: 10, binding: "tetris.survival_ms");

        var actual = Assert.Single(persisted);
        Assert.Equal("outcome-1", actual.ObservationId);
        Assert.Equal("decision-1", actual.DecisionId);
        Assert.Equal(18_400, actual.Value.GetInt32());
        Assert.Equal(observedAt, actual.ObservedAt);
        Assert.Equal("session-1", actual.CorrelationAttributes["sessionId"].GetString());
        Assert.Empty(await store.ListOutcomesAsync(Scope, limit: 10, binding: "other.binding"));
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
            + "DROP TABLE evidence_outcome_observations;");
        Assert.False(await store.IsAvailableAsync());
    }

    private static DecisionObservation DecisionObservation(
        string decisionId,
        DateTimeOffset observedAt,
        int result) =>
        new(
            Scope,
            decisionId,
            "tetris.dropInterval",
            ContractDigest,
            ExecutableDigest,
            JsonSerializer.SerializeToElement(result),
            ResultHash,
            "rule",
            "high-pressure",
            new Dictionary<string, JsonElement>
            {
                ["sessionId"] = JsonSerializer.SerializeToElement("session-1"),
                ["level"] = JsonSerializer.SerializeToElement(9)
            },
            observedAt);

    private static OutcomeObservation OutcomeObservation(
        string observationId,
        string binding,
        DateTimeOffset observedAt,
        int value) =>
        new(
            Scope,
            observationId,
            binding,
            JsonSerializer.SerializeToElement(value),
            "decision-1",
            "tetris.dropInterval",
            ContractDigest,
            new Dictionary<string, JsonElement>
            {
                ["sessionId"] = JsonSerializer.SerializeToElement("session-1")
            },
            observedAt);

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
