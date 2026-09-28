using System.Globalization;
using System.Text.Json;
using Flaggo.Contract;
using Microsoft.Data.Sqlite;

namespace Flaggo.EvidenceStore;

public sealed class SqliteEvidenceStore : IEvidenceStore
{
    private const string ComponentName = "evidence-store";
    private const int SchemaVersion = 1;
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

            CREATE TABLE IF NOT EXISTS evidence_decision_observations (
                application TEXT NOT NULL,
                environment TEXT NOT NULL,
                decision_id TEXT NOT NULL,
                contract_name TEXT NOT NULL,
                contract_digest TEXT NOT NULL,
                executable_digest TEXT NOT NULL,
                result_json BLOB NOT NULL,
                result_hash TEXT NOT NULL,
                evaluation_source TEXT NOT NULL
                    CHECK(evaluation_source IN ('rule', 'default')),
                evaluation_rule TEXT NULL,
                correlation_json BLOB NOT NULL,
                observed_at TEXT NOT NULL,
                PRIMARY KEY(application, environment, decision_id)
            );

            CREATE INDEX IF NOT EXISTS ix_evidence_decisions_contract_time
            ON evidence_decision_observations(
                application,
                environment,
                contract_name,
                observed_at DESC,
                decision_id DESC
            );

            CREATE TABLE IF NOT EXISTS evidence_outcome_observations (
                application TEXT NOT NULL,
                environment TEXT NOT NULL,
                observation_id TEXT NOT NULL,
                binding TEXT NOT NULL,
                value_json BLOB NOT NULL,
                decision_id TEXT NULL,
                contract_name TEXT NULL,
                contract_digest TEXT NULL,
                correlation_json BLOB NOT NULL,
                observed_at TEXT NOT NULL,
                PRIMARY KEY(application, environment, observation_id)
            );

            CREATE INDEX IF NOT EXISTS ix_evidence_outcomes_binding_time
            ON evidence_outcome_observations(
                application,
                environment,
                binding,
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

    public async Task<EvidenceObservationWriteResult> PutDecisionAsync(
        DecisionObservation observation,
        CancellationToken cancellationToken = default)
    {
        ValidateDecision(observation);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            INSERT INTO evidence_decision_observations(
                application,
                environment,
                decision_id,
                contract_name,
                contract_digest,
                executable_digest,
                result_json,
                result_hash,
                evaluation_source,
                evaluation_rule,
                correlation_json,
                observed_at
            )
            VALUES (
                $application,
                $environment,
                $decisionId,
                $contractName,
                $contractDigest,
                $executableDigest,
                $resultJson,
                $resultHash,
                $evaluationSource,
                $evaluationRule,
                $correlationJson,
                $observedAt
            )
            ON CONFLICT(application, environment, decision_id) DO NOTHING;
            """;
        AddScope(command, observation.Scope);
        command.Parameters.AddWithValue("$decisionId", observation.DecisionId);
        command.Parameters.AddWithValue("$contractName", observation.ContractName);
        command.Parameters.AddWithValue("$contractDigest", observation.ContractDigest);
        command.Parameters.AddWithValue("$executableDigest", observation.ExecutableDigest);
        command.Parameters.Add("$resultJson", SqliteType.Blob).Value =
            JsonSerializer.SerializeToUtf8Bytes(observation.Result, JsonOptions);
        command.Parameters.AddWithValue("$resultHash", observation.ResultHash);
        command.Parameters.AddWithValue("$evaluationSource", observation.EvaluationSource);
        command.Parameters.AddWithValue(
            "$evaluationRule",
            observation.EvaluationRule is null
                ? DBNull.Value
                : observation.EvaluationRule);
        command.Parameters.Add("$correlationJson", SqliteType.Blob).Value =
            SerializeCorrelation(observation.CorrelationAttributes);
        command.Parameters.AddWithValue("$observedAt", FormatTime(observation.ObservedAt));
        var rows = await command.ExecuteNonQueryAsync(cancellationToken);
        return rows == 1
            ? EvidenceObservationWriteResult.Created
            : EvidenceObservationWriteResult.Existing;
    }

    public async Task<EvidenceObservationWriteResult> PutOutcomeAsync(
        OutcomeObservation observation,
        CancellationToken cancellationToken = default)
    {
        ValidateOutcome(observation);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            INSERT INTO evidence_outcome_observations(
                application,
                environment,
                observation_id,
                binding,
                value_json,
                decision_id,
                contract_name,
                contract_digest,
                correlation_json,
                observed_at
            )
            VALUES (
                $application,
                $environment,
                $observationId,
                $binding,
                $valueJson,
                $decisionId,
                $contractName,
                $contractDigest,
                $correlationJson,
                $observedAt
            )
            ON CONFLICT(application, environment, observation_id) DO NOTHING;
            """;
        AddScope(command, observation.Scope);
        command.Parameters.AddWithValue("$observationId", observation.ObservationId);
        command.Parameters.AddWithValue("$binding", observation.Binding);
        command.Parameters.Add("$valueJson", SqliteType.Blob).Value =
            JsonSerializer.SerializeToUtf8Bytes(observation.Value, JsonOptions);
        command.Parameters.AddWithValue(
            "$decisionId",
            observation.DecisionId is null
                ? DBNull.Value
                : observation.DecisionId);
        command.Parameters.AddWithValue(
            "$contractName",
            observation.ContractName is null
                ? DBNull.Value
                : observation.ContractName);
        command.Parameters.AddWithValue(
            "$contractDigest",
            observation.ContractDigest is null
                ? DBNull.Value
                : observation.ContractDigest);
        command.Parameters.Add("$correlationJson", SqliteType.Blob).Value =
            SerializeCorrelation(observation.CorrelationAttributes);
        command.Parameters.AddWithValue("$observedAt", FormatTime(observation.ObservedAt));
        var rows = await command.ExecuteNonQueryAsync(cancellationToken);
        return rows == 1
            ? EvidenceObservationWriteResult.Created
            : EvidenceObservationWriteResult.Existing;
    }

    public async Task<IReadOnlyList<DecisionObservation>> ListDecisionsAsync(
        DecisionScope scope,
        int limit,
        string? contractName = null,
        CancellationToken cancellationToken = default)
    {
        ValidateScope(scope);
        ValidateLimit(limit);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT decision_id, contract_name, contract_digest, executable_digest,
                   result_json, result_hash, evaluation_source, evaluation_rule,
                   correlation_json, observed_at
            FROM evidence_decision_observations
            WHERE application = $application
              AND environment = $environment
              AND ($contractName IS NULL OR contract_name = $contractName)
            ORDER BY observed_at DESC, decision_id DESC
            LIMIT $limit;
            """;
        AddScope(command, scope);
        command.Parameters.AddWithValue(
            "$contractName",
            contractName is null ? DBNull.Value : contractName);
        command.Parameters.AddWithValue("$limit", limit);
        var observations = new List<DecisionObservation>();
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            observations.Add(ReadDecision(reader, scope));
        }
        return observations;
    }

    public async Task<IReadOnlyList<OutcomeObservation>> ListOutcomesAsync(
        DecisionScope scope,
        int limit,
        string? binding = null,
        CancellationToken cancellationToken = default)
    {
        ValidateScope(scope);
        ValidateLimit(limit);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT observation_id, binding, value_json, decision_id,
                   contract_name, contract_digest, correlation_json, observed_at
            FROM evidence_outcome_observations
            WHERE application = $application
              AND environment = $environment
              AND ($binding IS NULL OR binding = $binding)
            ORDER BY observed_at DESC, observation_id DESC
            LIMIT $limit;
            """;
        AddScope(command, scope);
        command.Parameters.AddWithValue("$binding", binding is null ? DBNull.Value : binding);
        command.Parameters.AddWithValue("$limit", limit);
        var observations = new List<OutcomeObservation>();
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            observations.Add(ReadOutcome(reader, scope));
        }
        return observations;
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

            await ProbeTableAsync(
                connection,
                """
                SELECT application, environment, decision_id, contract_name,
                       contract_digest, executable_digest, result_json, result_hash,
                       evaluation_source, evaluation_rule, correlation_json, observed_at
                FROM evidence_decision_observations
                LIMIT 0;
                """,
                cancellationToken);
            await ProbeTableAsync(
                connection,
                """
                SELECT application, environment, observation_id, binding, value_json,
                       decision_id, contract_name, contract_digest, correlation_json,
                       observed_at
                FROM evidence_outcome_observations
                LIMIT 0;
                """,
                cancellationToken);
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

    private static async Task ProbeTableAsync(
        SqliteConnection connection,
        string query,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.CommandText = query;
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        _ = await reader.ReadAsync(cancellationToken);
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

    private static void ValidateDecision(DecisionObservation observation)
    {
        ValidateScope(observation.Scope);
        ValidateIdentity(observation.DecisionId, nameof(observation.DecisionId));
        ValidateIdentity(observation.ContractName, nameof(observation.ContractName));
        ValidateDigest(observation.ContractDigest, nameof(observation.ContractDigest));
        ValidateDigest(observation.ExecutableDigest, nameof(observation.ExecutableDigest));
        ValidateDigest(observation.ResultHash, nameof(observation.ResultHash));
        if (observation.EvaluationSource is not ("rule" or "default"))
        {
            throw new ArgumentException("Evaluation source must be 'rule' or 'default'.", nameof(observation));
        }
        if (observation.EvaluationSource == "rule")
        {
            ValidateIdentity(observation.EvaluationRule, nameof(observation.EvaluationRule));
        }
        if (observation.EvaluationSource == "default" && observation.EvaluationRule is not null)
        {
            throw new ArgumentException("Default evaluations cannot carry a rule name.", nameof(observation));
        }
        ValidateCorrelation(observation.CorrelationAttributes);
    }

    private static void ValidateOutcome(OutcomeObservation observation)
    {
        ValidateScope(observation.Scope);
        ValidateIdentity(observation.ObservationId, nameof(observation.ObservationId));
        ValidateIdentity(observation.Binding, nameof(observation.Binding));
        if (observation.DecisionId is not null)
        {
            ValidateIdentity(observation.DecisionId, nameof(observation.DecisionId));
        }
        if (observation.ContractName is not null)
        {
            ValidateIdentity(observation.ContractName, nameof(observation.ContractName));
        }
        if (observation.ContractDigest is not null)
        {
            ValidateDigest(observation.ContractDigest, nameof(observation.ContractDigest));
        }
        ValidateCorrelation(observation.CorrelationAttributes);
    }

    private static void ValidateScope(DecisionScope scope)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Application);
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Environment);
    }

    private static void ValidateIdentity(string? value, string parameterName)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(value, parameterName);
        if (value.Length > 512 || value.Contains('\r', StringComparison.Ordinal)
            || value.Contains('\n', StringComparison.Ordinal))
        {
            throw new ArgumentException("Evidence identity values must be single-line strings up to 512 characters.", parameterName);
        }
    }

    private static void ValidateDigest(string value, string parameterName)
    {
        ValidateIdentity(value, parameterName);
        if (value.Length != 71 || !value.StartsWith("sha256:", StringComparison.Ordinal))
        {
            throw new ArgumentException("Digest values must use the sha256:<64 hex> form.", parameterName);
        }
    }

    private static void ValidateCorrelation(
        IReadOnlyDictionary<string, JsonElement> correlation)
    {
        ArgumentNullException.ThrowIfNull(correlation);
        foreach (var (name, value) in correlation)
        {
            ValidateIdentity(name, nameof(correlation));
            if (value.ValueKind is JsonValueKind.Object or JsonValueKind.Array)
            {
                throw new ArgumentException("Correlation values must be scalar JSON values.", nameof(correlation));
            }
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

    private static DecisionObservation ReadDecision(
        SqliteDataReader reader,
        DecisionScope scope) =>
        new(
            scope,
            reader.GetString(0),
            reader.GetString(1),
            reader.GetString(2),
            reader.GetString(3),
            ReadJson(reader, 4),
            reader.GetString(5),
            reader.GetString(6),
            reader.IsDBNull(7) ? null : reader.GetString(7),
            ReadCorrelation(reader, 8),
            ParseTime(reader.GetString(9)));

    private static OutcomeObservation ReadOutcome(
        SqliteDataReader reader,
        DecisionScope scope) =>
        new(
            scope,
            reader.GetString(0),
            reader.GetString(1),
            ReadJson(reader, 2),
            reader.IsDBNull(3) ? null : reader.GetString(3),
            reader.IsDBNull(4) ? null : reader.GetString(4),
            reader.IsDBNull(5) ? null : reader.GetString(5),
            ReadCorrelation(reader, 6),
            ParseTime(reader.GetString(7)));

    private static JsonElement ReadJson(SqliteDataReader reader, int ordinal)
    {
        using var document = JsonDocument.Parse((byte[])reader.GetValue(ordinal));
        return document.RootElement.Clone();
    }

    private static IReadOnlyDictionary<string, JsonElement> ReadCorrelation(
        SqliteDataReader reader,
        int ordinal)
    {
        using var document = JsonDocument.Parse((byte[])reader.GetValue(ordinal));
        var result = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var property in document.RootElement.EnumerateObject())
        {
            result[property.Name] = property.Value.Clone();
        }
        return result;
    }

    private static byte[] SerializeCorrelation(
        IReadOnlyDictionary<string, JsonElement> correlation) =>
        JsonSerializer.SerializeToUtf8Bytes(correlation, JsonOptions);

    private static string FormatTime(DateTimeOffset value) =>
        value.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture);

    private static DateTimeOffset ParseTime(string value) =>
        DateTimeOffset.ParseExact(
            value,
            "O",
            CultureInfo.InvariantCulture,
            DateTimeStyles.RoundtripKind);
}
