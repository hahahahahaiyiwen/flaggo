using System.Data;
using System.Globalization;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

namespace Flaggo.ExecutableStore;

public sealed class SqliteExecutableStore : IExecutableStore
{
    private const string ComponentName = "executable-store";
    private const int SchemaVersion = 4;
    private readonly string _connectionString;
    private readonly IExpressionCanonicalizer? _expressionCanonicalizer;
    private readonly TimeProvider _timeProvider;

    public SqliteExecutableStore(
        string connectionString,
        IExpressionCanonicalizer? expressionCanonicalizer = null,
        TimeProvider? timeProvider = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(connectionString);
        _connectionString = connectionString;
        _expressionCanonicalizer = expressionCanonicalizer;
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using (var versionTable = connection.CreateCommand())
        {
            versionTable.CommandText =
            """
            CREATE TABLE IF NOT EXISTS flaggo_schema_versions (
                component TEXT PRIMARY KEY,
                version INTEGER NOT NULL
            );
            """;
            await versionTable.ExecuteNonQueryAsync(cancellationToken);
        }

        await using (var readVersion = connection.CreateCommand())
        {
            readVersion.CommandText =
                "SELECT version FROM flaggo_schema_versions WHERE component = $component;";
            readVersion.Parameters.AddWithValue("$component", ComponentName);
            var storedVersion = await readVersion.ExecuteScalarAsync(cancellationToken);
            if (storedVersion is not null
                && Convert.ToInt32(storedVersion, CultureInfo.InvariantCulture) != SchemaVersion)
            {
                throw new InvalidOperationException(
                    $"Unsupported {ComponentName} schema version {storedVersion}; "
                    + $"expected {SchemaVersion}.");
            }
        }

        await using var command = connection.CreateCommand();
        command.CommandText =
            """

            INSERT INTO flaggo_schema_versions(component, version)
            VALUES ($component, $version)
            ON CONFLICT(component) DO NOTHING;

            CREATE TABLE IF NOT EXISTS decision_executables (
                executable_digest TEXT PRIMARY KEY,
                contract_digest TEXT NOT NULL,
                executable_json BLOB NOT NULL,
                checked_executable_json BLOB NOT NULL,
                provenance_json BLOB NULL,
                lifecycle_state TEXT NOT NULL
                    CHECK(lifecycle_state IN ('candidate', 'active', 'inactive')),
                state_version INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                activated_at TEXT NULL
            );

            CREATE INDEX IF NOT EXISTS ix_executables_contract
            ON decision_executables(contract_digest);

            CREATE UNIQUE INDEX IF NOT EXISTS ux_executables_one_active
            ON decision_executables(contract_digest)
            WHERE lifecycle_state = 'active';

            CREATE TABLE IF NOT EXISTS analysis_candidate_admissions (
                workspace_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                contract_name TEXT NOT NULL,
                contract_digest TEXT NOT NULL,
                executable_digest TEXT NOT NULL,
                evidence_cutoff TEXT NOT NULL,
                evidence_watermark INTEGER NOT NULL CHECK(evidence_watermark > 0),
                analysis_manifest_digest TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY(workspace_id, cycle_id),
                FOREIGN KEY(executable_digest)
                    REFERENCES decision_executables(executable_digest)
            );

            CREATE INDEX IF NOT EXISTS ix_analysis_candidates_contract
            ON analysis_candidate_admissions(contract_name, contract_digest, created_at);
            """;
        command.Parameters.AddWithValue("$component", ComponentName);
        command.Parameters.AddWithValue("$version", SchemaVersion);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<ExecutableStoreWriteResult> PutCandidateAsync(
        StoredExecutable executable,
        CancellationToken cancellationToken = default)
    {
        ValidateStoredExecutable(executable);
        await using var connection = await OpenAsync(cancellationToken);
        return await InsertCandidateAsync(
            connection,
            transaction: null,
            executable,
            cancellationToken);
    }

    public async Task<AnalysisCandidateStoreResult> PutAnalysisCandidateAsync(
        StoredExecutable executable,
        AnalysisCandidateAdmission admission,
        CancellationToken cancellationToken = default)
    {
        ValidateStoredExecutable(executable);
        ValidateAdmission(executable, admission);
        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = connection.BeginTransaction(
            IsolationLevel.Serializable,
            deferred: false);
        var existing = await ReadAdmissionAsync(
            connection,
            (SqliteTransaction)transaction,
            admission.WorkspaceId,
            admission.CycleId,
            cancellationToken);
        if (existing is not null)
        {
            if (!AdmissionsMatch(existing, admission))
            {
                throw new CandidateAdmissionConflictException(
                    admission.WorkspaceId,
                    admission.CycleId);
            }

            var admittedState = await ReadLifecycleStateAsync(
                connection,
                (SqliteTransaction)transaction,
                existing.ExecutableDigest,
                cancellationToken);
            if (admittedState is not ExecutableLifecycleState.Candidate)
            {
                throw new CandidateLifecycleConflictException(
                    existing.ExecutableDigest,
                    admittedState
                        ?? throw new InvalidDataException(
                            "Candidate admission references a missing executable."));
            }

            await transaction.CommitAsync(cancellationToken);
            return new AnalysisCandidateStoreResult(
                existing.ExecutableDigest,
                existing.CreatedAt,
                false);
        }

        var existingState = await ReadLifecycleStateAsync(
            connection,
            (SqliteTransaction)transaction,
            executable.ExecutableDigest,
            cancellationToken);
        if (existingState is not null
            && existingState != ExecutableLifecycleState.Candidate)
        {
            throw new CandidateLifecycleConflictException(
                executable.ExecutableDigest,
                existingState.Value);
        }

        await InsertCandidateAsync(
            connection,
            (SqliteTransaction)transaction,
            executable,
            cancellationToken);
        await using var command = connection.CreateCommand();
        command.Transaction = (SqliteTransaction)transaction;
        command.CommandText =
            """
            INSERT INTO analysis_candidate_admissions(
                workspace_id,
                cycle_id,
                attempt_id,
                contract_name,
                contract_digest,
                executable_digest,
                evidence_cutoff,
                evidence_watermark,
                analysis_manifest_digest,
                created_at
            )
            VALUES (
                $workspaceId,
                $cycleId,
                $attemptId,
                $contractName,
                $contractDigest,
                $executableDigest,
                $evidenceCutoff,
                $evidenceWatermark,
                $analysisManifestDigest,
                $createdAt
            );
            """;
        command.Parameters.AddWithValue("$workspaceId", admission.WorkspaceId);
        command.Parameters.AddWithValue("$cycleId", admission.CycleId);
        command.Parameters.AddWithValue("$attemptId", admission.AttemptId);
        command.Parameters.AddWithValue("$contractName", admission.ContractName);
        command.Parameters.AddWithValue("$contractDigest", admission.ContractDigest);
        command.Parameters.AddWithValue("$executableDigest", admission.ExecutableDigest);
        command.Parameters.AddWithValue("$evidenceCutoff", FormatTime(admission.EvidenceCutoff));
        command.Parameters.AddWithValue("$evidenceWatermark", admission.EvidenceWatermark);
        command.Parameters.AddWithValue(
            "$analysisManifestDigest",
            admission.AnalysisManifestDigest);
        command.Parameters.AddWithValue("$createdAt", FormatTime(admission.CreatedAt));
        await command.ExecuteNonQueryAsync(cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        return new AnalysisCandidateStoreResult(
            admission.ExecutableDigest,
            admission.CreatedAt,
            true);
    }

    private async Task<ExecutableStoreWriteResult> InsertCandidateAsync(
        SqliteConnection connection,
        SqliteTransaction? transaction,
        StoredExecutable executable,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            INSERT INTO decision_executables(
                executable_digest,
                contract_digest,
                executable_json,
                checked_executable_json,
                provenance_json,
                lifecycle_state,
                state_version,
                created_at,
                activated_at
            )
            VALUES (
                $executableDigest,
                $contractDigest,
                $executableJson,
                $checkedExecutableJson,
                $provenanceJson,
                'candidate',
                0,
                $createdAt,
                NULL
            )
            ON CONFLICT(executable_digest)
            DO NOTHING;
            """;
        command.Parameters.AddWithValue("$executableDigest", executable.ExecutableDigest);
        command.Parameters.AddWithValue("$contractDigest", executable.Executable.ContractDigest);
        command.Parameters.Add("$executableJson", SqliteType.Blob).Value =
            StrictJson.SerializeToUtf8Bytes(executable.Executable);
        command.Parameters.Add("$checkedExecutableJson", SqliteType.Blob).Value =
            JsonSerializer.SerializeToUtf8Bytes(
                executable.CheckedExecutable,
                StrictJson.Options);
        command.Parameters.Add("$provenanceJson", SqliteType.Blob).Value =
            executable.Provenance is null
                ? DBNull.Value
                : JsonSerializer.SerializeToUtf8Bytes(
                    executable.Provenance.Value,
                    StrictJson.Options);
        command.Parameters.AddWithValue("$createdAt", FormatTime(executable.CreatedAt));
        var rows = await command.ExecuteNonQueryAsync(cancellationToken);
        return rows == 1 ? ExecutableStoreWriteResult.Created : ExecutableStoreWriteResult.Existing;
    }

    public async Task<StoredExecutable?> GetAsync(
        string executableDigest,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT contract_digest, executable_json, checked_executable_json,
                   provenance_json, lifecycle_state, state_version, created_at, activated_at
            FROM decision_executables
            WHERE executable_digest = $executableDigest;
            """;
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        return await reader.ReadAsync(cancellationToken)
            ? ReadExecutable(reader, executableDigest)
            : null;
    }

    public async Task<StoredExecutable?> GetActiveAsync(
        string contractDigest,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT executable_digest, contract_digest, executable_json,
                   checked_executable_json, provenance_json, lifecycle_state,
                   state_version, created_at, activated_at
            FROM decision_executables
            WHERE contract_digest = $contractDigest
              AND lifecycle_state = 'active';
            """;
        command.Parameters.AddWithValue("$contractDigest", contractDigest);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        return await reader.ReadAsync(cancellationToken)
            ? ReadExecutable(reader, reader.GetString(0), columnOffset: 1)
            : null;
    }

    public async Task<ActivationResult> ActivateAsync(
        string contractDigest,
        string executableDigest,
        string? expectedActiveExecutableDigest = null,
        CancellationToken cancellationToken = default) =>
        await ActivateCoreAsync(
            contractDigest,
            executableDigest,
            enforceExpectedActive: expectedActiveExecutableDigest is not null,
            expectedActiveExecutableDigest,
            cancellationToken: cancellationToken);

    public async Task<ActivationResult> ActivateIfNoneAsync(
        string contractDigest,
        string executableDigest,
        CancellationToken cancellationToken = default) =>
        await ActivateCoreAsync(
            contractDigest,
            executableDigest,
            enforceExpectedActive: true,
            expectedActiveExecutableDigest: null,
            cancellationToken: cancellationToken);

    private async Task<ActivationResult> ActivateCoreAsync(
        string contractDigest,
        string executableDigest,
        bool enforceExpectedActive,
        string? expectedActiveExecutableDigest,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = connection.BeginTransaction(
            IsolationLevel.Serializable,
            deferred: false);

        var selectedContractDigest = await ReadSelectedContractDigest(
            connection,
            (SqliteTransaction)transaction,
            executableDigest,
            cancellationToken);
        if (selectedContractDigest is null)
        {
            throw new ExecutableNotFoundException(executableDigest);
        }

        if (!string.Equals(selectedContractDigest, contractDigest, StringComparison.Ordinal))
        {
            throw new ExecutableContractMismatchException(
                executableDigest,
                contractDigest,
                selectedContractDigest);
        }

        var current = await ReadActiveIdentity(
            connection,
            (SqliteTransaction)transaction,
            contractDigest,
            cancellationToken);
        if (enforceExpectedActive
            && !string.Equals(
                current?.ExecutableDigest,
                expectedActiveExecutableDigest,
                StringComparison.Ordinal))
        {
            throw new ActivationConflictException(
                expectedActiveExecutableDigest,
                current?.ExecutableDigest);
        }

        if (current is { } active
            && string.Equals(active.ExecutableDigest, executableDigest, StringComparison.Ordinal))
        {
            await transaction.CommitAsync(cancellationToken);
            return new ActivationResult(
                executableDigest,
                active.ExecutableDigest,
                active.StateVersion,
                false);
        }

        await using var deactivate = connection.CreateCommand();
        deactivate.Transaction = (SqliteTransaction)transaction;
        deactivate.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'inactive',
                state_version = state_version + 1
            WHERE contract_digest = $contractDigest
              AND lifecycle_state = 'active';
            """;
        deactivate.Parameters.AddWithValue("$contractDigest", contractDigest);
        await deactivate.ExecuteNonQueryAsync(cancellationToken);

        var nextVersion = (current?.StateVersion ?? 0) + 1;
        await using var activate = connection.CreateCommand();
        activate.Transaction = (SqliteTransaction)transaction;
        activate.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'active',
                state_version = $stateVersion,
                activated_at = $activatedAt
            WHERE executable_digest = $executableDigest;
            """;
        activate.Parameters.AddWithValue("$executableDigest", executableDigest);
        activate.Parameters.AddWithValue("$stateVersion", nextVersion);
        activate.Parameters.AddWithValue("$activatedAt", FormatTime(_timeProvider.GetUtcNow()));
        if (await activate.ExecuteNonQueryAsync(cancellationToken) != 1)
        {
            throw new ExecutableNotFoundException(executableDigest);
        }

        await transaction.CommitAsync(cancellationToken);
        return new ActivationResult(
            executableDigest,
            current?.ExecutableDigest,
            nextVersion,
            true);
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
                SELECT executable_digest, contract_digest,
                       executable_json, checked_executable_json, provenance_json,
                       lifecycle_state, state_version, created_at, activated_at
                FROM decision_executables
                LIMIT 0;
                """;
            await using var reader = await command.ExecuteReaderAsync(cancellationToken);
            _ = await reader.ReadAsync(cancellationToken);
            await reader.DisposeAsync();

            await using var admissionCommand = connection.CreateCommand();
            admissionCommand.CommandText =
                """
                SELECT workspace_id, cycle_id, attempt_id, contract_name,
                       contract_digest, executable_digest, evidence_cutoff,
                       evidence_watermark, analysis_manifest_digest, created_at
                FROM analysis_candidate_admissions
                LIMIT 0;
                """;
            await using var admissionReader =
                await admissionCommand.ExecuteReaderAsync(cancellationToken);
            _ = await admissionReader.ReadAsync(cancellationToken);
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

    private void ValidateStoredExecutable(StoredExecutable stored)
    {
        if (stored.State != ExecutableLifecycleState.Candidate
            || stored.StateVersion != 0
            || stored.ActivatedAt is not null)
        {
            throw new ArgumentException(
                "New executables must enter the store as version-zero candidates.",
                nameof(stored));
        }

        if (stored.CheckedExecutable.Rules.Count != stored.Executable.Rules.Count
            || stored.CheckedExecutable.Rules
                .Where((rule, index) =>
                    !string.Equals(
                        rule.Name,
                        stored.Executable.Rules[index].Name,
                        StringComparison.Ordinal))
                .Any())
        {
            throw new ArgumentException(
                "Checked executable rules must align with executable rules.",
                nameof(stored));
        }

        var digest = ContractDigests.ComputeExecutableDigest(
            stored.Executable,
            _expressionCanonicalizer);
        if (!string.Equals(digest, stored.ExecutableDigest, StringComparison.Ordinal))
        {
            throw new ArgumentException(
                $"Executable digest mismatch: expected {digest}, received {stored.ExecutableDigest}.",
                nameof(stored));
        }

        if (_expressionCanonicalizer is not null
            && !string.Equals(
                ContractDigests.ComputeExecutableDigest(stored.Executable),
                stored.ExecutableDigest,
                StringComparison.Ordinal))
        {
            throw new ArgumentException(
                "Executable expressions must use their canonical stored form.",
                nameof(stored));
        }
    }

    private static void ValidateAdmission(
        StoredExecutable executable,
        AnalysisCandidateAdmission admission)
    {
        if (!string.Equals(
                admission.ContractDigest,
                executable.Executable.ContractDigest,
                StringComparison.Ordinal)
            || !string.Equals(
                admission.ExecutableDigest,
                executable.ExecutableDigest,
                StringComparison.Ordinal))
        {
            throw new ArgumentException(
                "Candidate admission identity must match the stored executable.",
                nameof(admission));
        }

        if (string.IsNullOrWhiteSpace(admission.WorkspaceId)
            || string.IsNullOrWhiteSpace(admission.CycleId)
            || string.IsNullOrWhiteSpace(admission.AttemptId)
            || string.IsNullOrWhiteSpace(admission.ContractName)
            || admission.EvidenceWatermark <= 0)
        {
            throw new ArgumentException(
                "Candidate admission identity and positive evidence watermark are required.",
                nameof(admission));
        }
    }

    private static bool AdmissionsMatch(
        AnalysisCandidateAdmission existing,
        AnalysisCandidateAdmission requested) =>
        string.Equals(existing.WorkspaceId, requested.WorkspaceId, StringComparison.Ordinal)
        && string.Equals(existing.CycleId, requested.CycleId, StringComparison.Ordinal)
        && string.Equals(existing.AttemptId, requested.AttemptId, StringComparison.Ordinal)
        && string.Equals(existing.ContractName, requested.ContractName, StringComparison.Ordinal)
        && string.Equals(
            existing.ContractDigest,
            requested.ContractDigest,
            StringComparison.Ordinal)
        && string.Equals(
            existing.ExecutableDigest,
            requested.ExecutableDigest,
            StringComparison.Ordinal)
        && existing.EvidenceCutoff == requested.EvidenceCutoff
        && existing.EvidenceWatermark == requested.EvidenceWatermark
        && string.Equals(
            existing.AnalysisManifestDigest,
            requested.AnalysisManifestDigest,
            StringComparison.Ordinal);

    private static async Task<AnalysisCandidateAdmission?> ReadAdmissionAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string workspaceId,
        string cycleId,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT attempt_id, contract_name, contract_digest, executable_digest,
                   evidence_cutoff, evidence_watermark, analysis_manifest_digest, created_at
            FROM analysis_candidate_admissions
            WHERE workspace_id = $workspaceId
              AND cycle_id = $cycleId;
            """;
        command.Parameters.AddWithValue("$workspaceId", workspaceId);
        command.Parameters.AddWithValue("$cycleId", cycleId);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        if (!await reader.ReadAsync(cancellationToken))
        {
            return null;
        }

        return new AnalysisCandidateAdmission(
            workspaceId,
            cycleId,
            reader.GetString(0),
            reader.GetString(1),
            reader.GetString(2),
            reader.GetString(3),
            DateTimeOffset.Parse(
                reader.GetString(4),
                CultureInfo.InvariantCulture,
                DateTimeStyles.RoundtripKind),
            reader.GetInt64(5),
            reader.GetString(6),
            DateTimeOffset.Parse(
                reader.GetString(7),
                CultureInfo.InvariantCulture,
                DateTimeStyles.RoundtripKind));
    }

    private static async Task<ExecutableLifecycleState?> ReadLifecycleStateAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string executableDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            "SELECT lifecycle_state FROM decision_executables "
            + "WHERE executable_digest = $executableDigest;";
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        var value = await command.ExecuteScalarAsync(cancellationToken) as string;
        return value switch
        {
            null => null,
            "candidate" => ExecutableLifecycleState.Candidate,
            "active" => ExecutableLifecycleState.Active,
            "inactive" => ExecutableLifecycleState.Inactive,
            var invalid => throw new InvalidDataException(
                $"Unknown executable lifecycle state '{invalid}'.")
        };
    }

    private static async Task<string?> ReadSelectedContractDigest(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string executableDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT contract_digest
            FROM decision_executables
            WHERE executable_digest = $executableDigest;
            """;
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        return await command.ExecuteScalarAsync(cancellationToken) as string;
    }

    private static async Task<(string ExecutableDigest, long StateVersion)?> ReadActiveIdentity(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT executable_digest, state_version
            FROM decision_executables
            WHERE contract_digest = $contractDigest
              AND lifecycle_state = 'active';
            """;
        command.Parameters.AddWithValue("$contractDigest", contractDigest);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        return await reader.ReadAsync(cancellationToken)
            ? (reader.GetString(0), reader.GetInt64(1))
            : null;
    }

    private StoredExecutable ReadExecutable(
        SqliteDataReader reader,
        string executableDigest,
        int columnOffset = 0)
    {
        var contractDigest = reader.GetString(columnOffset);
        var executable = StrictJson.Deserialize<DecisionExecutable>(
            (byte[])reader.GetValue(columnOffset + 1));
        if (!string.Equals(
                executable.ContractDigest,
                contractDigest,
                StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                $"Stored executable contract digest '{contractDigest}' does not match payload "
                + $"'{executable.ContractDigest}'.");
        }

        var computedDigest = ContractDigests.ComputeExecutableDigest(executable);
        if (!string.Equals(computedDigest, executableDigest, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                $"Stored executable digest '{executableDigest}' does not match payload "
                + $"'{computedDigest}'.");
        }

        var checkedExecutable =
            JsonSerializer.Deserialize<CheckedDecisionExecutable>(
                (byte[])reader.GetValue(columnOffset + 2),
                StrictJson.Options)
            ?? throw new InvalidDataException(
                "Stored checked executable representation is null.");
        if (checkedExecutable.Rules.Count != executable.Rules.Count
            || checkedExecutable.Rules
                .Where((rule, index) =>
                    !string.Equals(
                        rule.Name,
                        executable.Rules[index].Name,
                        StringComparison.Ordinal))
                .Any())
        {
            throw new InvalidDataException(
                "Stored checked executable rules do not match executable rules.");
        }

        var provenance = reader.IsDBNull(columnOffset + 3)
            ? (JsonElement?)null
            : StrictJson.Deserialize<JsonElement>((byte[])reader.GetValue(columnOffset + 3));
        var state = reader.GetString(columnOffset + 4) switch
        {
            "candidate" => ExecutableLifecycleState.Candidate,
            "active" => ExecutableLifecycleState.Active,
            "inactive" => ExecutableLifecycleState.Inactive,
            var invalid => throw new InvalidDataException(
                $"Unknown executable lifecycle state '{invalid}'.")
        };
        var createdAt = ParseTime(reader.GetString(columnOffset + 6));
        DateTimeOffset? activatedAt = reader.IsDBNull(columnOffset + 7)
            ? null
            : ParseTime(reader.GetString(columnOffset + 7));
        return new StoredExecutable(
            executableDigest,
            executable,
            checkedExecutable,
            state,
            reader.GetInt64(columnOffset + 5),
            createdAt,
            activatedAt,
            provenance);
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
