using System.Globalization;
using System.Text.Json;
using Flaggo.Contract;
using Microsoft.Data.Sqlite;

namespace Flaggo.EvidenceStore;

public sealed class SqliteEvidenceStore : IEvidenceStore
{
    private const string ComponentName = "evidence-store";
    private const int SchemaVersion = 2;
    private static readonly JsonSerializerOptions JsonOptions = StrictJson.Options;
    private readonly string _connectionString;

    public SqliteEvidenceStore(string connectionString)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(connectionString);
        _connectionString = connectionString;
    }

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
                component TEXT PRIMARY KEY,
                version INTEGER NOT NULL
            );

            INSERT INTO flaggo_schema_versions(component, version)
            VALUES ($component, $version)
            ON CONFLICT(component) DO NOTHING;

            CREATE TABLE IF NOT EXISTS evidence_telemetry_records (
                application TEXT NOT NULL,
                environment TEXT NOT NULL,
                observation_id TEXT NOT NULL,
                telemetry_type TEXT NOT NULL,
                signal TEXT NOT NULL,
                payload_json BLOB NOT NULL,
                observed_at TEXT NOT NULL,
                received_at TEXT NOT NULL,
                PRIMARY KEY(application, environment, observation_id)
            );

            CREATE INDEX IF NOT EXISTS ix_evidence_telemetry_signal_time
            ON evidence_telemetry_records(
                application,
                environment,
                telemetry_type,
                signal,
                observed_at DESC,
                observation_id DESC
            );
            """;
        command.Parameters.AddWithValue("$component", ComponentName);
        command.Parameters.AddWithValue("$version", SchemaVersion);
        await command.ExecuteNonQueryAsync(cancellationToken);

        await using var versionCommand = connection.CreateCommand();
        versionCommand.CommandText =
            "SELECT version FROM flaggo_schema_versions WHERE component = $component;";
        versionCommand.Parameters.AddWithValue("$component", ComponentName);
        var version = Convert.ToInt32(
            await versionCommand.ExecuteScalarAsync(cancellationToken),
            CultureInfo.InvariantCulture);
        if (version != SchemaVersion)
        {
            throw new InvalidOperationException(
                $"Unsupported {ComponentName} schema version {version}; expected {SchemaVersion}.");
        }
    }

    public async Task<EvidenceTelemetryWriteResult> PutTelemetryBatchAsync(
        IReadOnlyList<EvidenceTelemetryRecord> records,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(records);
        if (records.Count == 0)
        {
            return new EvidenceTelemetryWriteResult(Created: 0, Existing: 0);
        }

        foreach (var record in records)
        {
            ValidateRecord(record);
        }

        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(cancellationToken);
        var created = 0;
        foreach (var record in records)
        {
            await using var command = connection.CreateCommand();
            command.Transaction = (SqliteTransaction)transaction;
            command.CommandText =
                """
                INSERT INTO evidence_telemetry_records(
                    application,
                    environment,
                    observation_id,
                    telemetry_type,
                    signal,
                    payload_json,
                    observed_at,
                    received_at
                )
                VALUES (
                    $application,
                    $environment,
                    $observationId,
                    $telemetryType,
                    $signal,
                    $payloadJson,
                    $observedAt,
                    $receivedAt
                )
                ON CONFLICT(application, environment, observation_id) DO NOTHING;
                """;
            AddScope(command, record.Scope);
            command.Parameters.AddWithValue("$observationId", record.ObservationId);
            command.Parameters.AddWithValue("$telemetryType", record.TelemetryType);
            command.Parameters.AddWithValue("$signal", record.Signal);
            command.Parameters.Add("$payloadJson", SqliteType.Blob).Value =
                JsonSerializer.SerializeToUtf8Bytes(record.Payload, JsonOptions);
            command.Parameters.AddWithValue("$observedAt", FormatTime(record.ObservedAt));
            command.Parameters.AddWithValue("$receivedAt", FormatTime(record.ReceivedAt));
            created += await command.ExecuteNonQueryAsync(cancellationToken);
        }
        await transaction.CommitAsync(cancellationToken);
        return new EvidenceTelemetryWriteResult(
            Created: created,
            Existing: records.Count - created);
    }

    public async Task<IReadOnlyList<EvidenceTelemetryRecord>> ListTelemetryAsync(
        DecisionScope scope,
        int limit,
        string? telemetryType = null,
        string? signal = null,
        CancellationToken cancellationToken = default)
    {
        ValidateScope(scope);
        ValidateLimit(limit);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT observation_id, telemetry_type, signal, payload_json, observed_at, received_at
            FROM evidence_telemetry_records
            WHERE application = $application
              AND environment = $environment
              AND ($telemetryType IS NULL OR telemetry_type = $telemetryType)
              AND ($signal IS NULL OR signal = $signal)
            ORDER BY observed_at DESC, observation_id DESC
            LIMIT $limit;
            """;
        AddScope(command, scope);
        command.Parameters.AddWithValue("$telemetryType", telemetryType is null ? DBNull.Value : telemetryType);
        command.Parameters.AddWithValue("$signal", signal is null ? DBNull.Value : signal);
        command.Parameters.AddWithValue("$limit", limit);
        var records = new List<EvidenceTelemetryRecord>();
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            records.Add(ReadRecord(reader, scope));
        }
        return records;
    }

    public async Task<bool> IsAvailableAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            await using var connection = await OpenAsync(cancellationToken);
            if (!await HasExpectedSchemaVersionAsync(connection, cancellationToken))
            {
                return false;
            }

            await using var command = connection.CreateCommand();
            command.CommandText =
                """
                SELECT application, environment, observation_id, telemetry_type, signal,
                       payload_json, observed_at, received_at
                FROM evidence_telemetry_records
                LIMIT 0;
                """;
            await using var reader = await command.ExecuteReaderAsync(cancellationToken);
            _ = await reader.ReadAsync(cancellationToken);
            return true;
        }
        catch (SqliteException)
        {
            return false;
        }
    }

    private static async Task<bool> HasExpectedSchemaVersionAsync(
        SqliteConnection connection,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.CommandText =
            "SELECT version FROM flaggo_schema_versions WHERE component = $component;";
        command.Parameters.AddWithValue("$component", ComponentName);
        return await command.ExecuteScalarAsync(cancellationToken) is long version
            && version == SchemaVersion;
    }

    private async Task<SqliteConnection> OpenAsync(CancellationToken cancellationToken)
    {
        var connection = new SqliteConnection(_connectionString);
        await connection.OpenAsync(cancellationToken);
        await using var pragma = connection.CreateCommand();
        pragma.CommandText = "PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;";
        await pragma.ExecuteNonQueryAsync(cancellationToken);
        return connection;
    }

    private static void ValidateRecord(EvidenceTelemetryRecord record)
    {
        ValidateScope(record.Scope);
        ValidateIdentity(record.ObservationId, nameof(record.ObservationId));
        ValidateIdentity(record.TelemetryType, nameof(record.TelemetryType));
        ValidateIdentity(record.Signal, nameof(record.Signal));
        if (record.Payload.ValueKind != JsonValueKind.Object)
        {
            throw new ArgumentException("Telemetry payload must be a JSON object.", nameof(record));
        }
    }

    private static void ValidateScope(DecisionScope scope)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Application);
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Environment);
    }

    private static void ValidateIdentity(string value, string parameterName)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(value, parameterName);
        if (value.Length > 512
            || value.Contains('\r', StringComparison.Ordinal)
            || value.Contains('\n', StringComparison.Ordinal))
        {
            throw new ArgumentException(
                "Evidence identity values must be single-line strings up to 512 characters.",
                parameterName);
        }
    }

    private static void ValidateLimit(int limit)
    {
        if (limit is < 1 or > 500)
        {
            throw new ArgumentOutOfRangeException(nameof(limit), "Limit must be from 1 to 500.");
        }
    }

    private static void AddScope(SqliteCommand command, DecisionScope scope)
    {
        command.Parameters.AddWithValue("$application", scope.Application);
        command.Parameters.AddWithValue("$environment", scope.Environment);
    }

    private static EvidenceTelemetryRecord ReadRecord(
        SqliteDataReader reader,
        DecisionScope scope)
    {
        using var payload = JsonDocument.Parse((byte[])reader.GetValue(3));
        return new EvidenceTelemetryRecord(
            scope,
            reader.GetString(0),
            reader.GetString(1),
            reader.GetString(2),
            payload.RootElement.Clone(),
            ParseTime(reader.GetString(4)),
            ParseTime(reader.GetString(5)));
    }

    private static string FormatTime(DateTimeOffset value) =>
        value.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture);

    private static DateTimeOffset ParseTime(string value) =>
        DateTimeOffset.ParseExact(
            value,
            "O",
            CultureInfo.InvariantCulture,
            DateTimeStyles.RoundtripKind);
}
