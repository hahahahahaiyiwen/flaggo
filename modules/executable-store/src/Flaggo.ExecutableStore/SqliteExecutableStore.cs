using System.Data;
using System.Globalization;
using System.Text.Json;
using Flaggo.Contract;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

namespace Flaggo.ExecutableStore;

public sealed class SqliteExecutableStore :
    IExecutableStore,
    IAnalysisCandidateActivationStore
{
    private const string ComponentName = "executable-store";
    private const int MaximumActivationPageSize = 1_000;
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

            CREATE INDEX IF NOT EXISTS ix_executables_analysis_candidates
            ON decision_executables(executable_digest, state_version)
            WHERE lifecycle_state = 'candidate';

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

            CREATE INDEX IF NOT EXISTS ix_analysis_candidates_executable
            ON analysis_candidate_admissions(
                executable_digest,
                contract_name,
                contract_digest,
                created_at DESC
            );
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
            if (admittedState is null)
            {
                throw new InvalidDataException(
                    "Candidate admission references a missing executable.");
            }

            await transaction.CommitAsync(cancellationToken);
            return new AnalysisCandidateStoreResult(
                existing.ExecutableDigest,
                existing.CreatedAt,
                admittedState.Value,
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
            ExecutableLifecycleState.Candidate,
            true);
    }

    public async Task<AnalysisCandidateActivationPage> ListPendingAsync(
        int pageSize,
        string? afterContractName = null,
        CancellationToken cancellationToken = default)
    {
        if (pageSize is < 1 or > MaximumActivationPageSize)
        {
            throw new ArgumentOutOfRangeException(
                nameof(pageSize),
                $"Page size must be between 1 and {MaximumActivationPageSize}.");
        }

        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            WITH ranked_candidates AS (
                SELECT admission.workspace_id,
                       admission.cycle_id,
                       admission.attempt_id,
                       admission.contract_name,
                       admission.contract_digest,
                       admission.executable_digest,
                       admission.evidence_cutoff,
                       admission.evidence_watermark,
                       admission.analysis_manifest_digest,
                       admission.created_at,
                       executable.state_version,
                       current.contract_digest AS current_contract_digest,
                       ROW_NUMBER() OVER (
                           PARTITION BY admission.contract_name
                           ORDER BY
                               CASE
                                   WHEN admission.contract_digest =
                                       current.contract_digest
                                   THEN 0
                                   ELSE 1
                               END,
                               admission.created_at DESC,
                               admission.executable_digest DESC,
                               admission.workspace_id DESC,
                               admission.cycle_id DESC
                       ) AS candidate_rank
                FROM decision_executables AS executable
                CROSS JOIN analysis_candidate_admissions AS admission
                LEFT JOIN decision_contract_current AS current
                  ON current.contract_name = admission.contract_name
                WHERE executable.lifecycle_state = 'candidate'
                  AND admission.executable_digest = executable.executable_digest
                  AND (
                      $afterContractName IS NULL
                      OR admission.contract_name > $afterContractName
                  )
            )
            SELECT candidate.workspace_id,
                   candidate.cycle_id,
                   candidate.attempt_id,
                   candidate.contract_name,
                   candidate.contract_digest,
                   candidate.executable_digest,
                   candidate.evidence_cutoff,
                   candidate.evidence_watermark,
                   candidate.analysis_manifest_digest,
                   candidate.created_at,
                   candidate.state_version,
                   active.executable_digest,
                   active.state_version
            FROM ranked_candidates AS candidate
            LEFT JOIN decision_executables AS active
              ON active.contract_digest = candidate.contract_digest
             AND active.lifecycle_state = 'active'
            WHERE candidate.candidate_rank = 1
            ORDER BY candidate.contract_name ASC
            LIMIT $limit;
            """;
        command.Parameters.AddWithValue(
            "$afterContractName",
            afterContractName is null ? DBNull.Value : afterContractName);
        command.Parameters.AddWithValue("$limit", pageSize + 1);
        var candidates = new List<AnalysisCandidateActivationSnapshot>(pageSize + 1);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            var admission = new AnalysisCandidateAdmission(
                reader.GetString(0),
                reader.GetString(1),
                reader.GetString(2),
                reader.GetString(3),
                reader.GetString(4),
                reader.GetString(5),
                ParseTime(reader.GetString(6)),
                reader.GetInt64(7),
                reader.GetString(8),
                ParseTime(reader.GetString(9)));
            var active = reader.IsDBNull(11)
                ? null
                : new ExecutableLifecycleToken(
                    reader.GetString(11),
                    reader.GetInt64(12));
            candidates.Add(new AnalysisCandidateActivationSnapshot(
                admission,
                reader.GetInt64(10),
                active));
        }

        string? nextContractName = null;
        if (candidates.Count > pageSize)
        {
            candidates.RemoveAt(pageSize);
            nextContractName = candidates[^1].Admission.ContractName;
        }

        return new AnalysisCandidateActivationPage(candidates, nextContractName);
    }

    public async Task<AnalysisCandidateResolutionResult> ResolveAsync(
        AnalysisCandidateResolutionCommand command,
        CancellationToken cancellationToken = default)
    {
        ValidateResolutionCommand(command);
        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = connection.BeginTransaction(
            IsolationLevel.Serializable,
            deferred: false);
        var sqliteTransaction = (SqliteTransaction)transaction;
        var candidate = await ReadResolutionCandidateAsync(
            connection,
            sqliteTransaction,
            command.Admission,
            cancellationToken);
        if (candidate is null)
        {
            throw new InvalidDataException(
                $"Analysis admission for Candidate "
                + $"'{command.Admission.ExecutableDigest}' does not exist.");
        }

        if (candidate.Value.State != ExecutableLifecycleState.Candidate)
        {
            await transaction.CommitAsync(cancellationToken);
            return new AnalysisCandidateResolutionResult(
                AnalysisCandidateResolutionOutcome.AlreadyResolved,
                command.Admission.ExecutableDigest,
                PreviousExecutableDigest: null,
                candidate.Value.StateVersion,
                SupersededCandidateCount: 0);
        }

        if (candidate.Value.StateVersion != command.ExpectedCandidateStateVersion
            || candidate.Value.CreatedAt != command.Admission.CreatedAt)
        {
            await transaction.CommitAsync(cancellationToken);
            return Yielded(command.Admission.ExecutableDigest);
        }

        var currentContractDigest = await ReadCurrentContractDigestAsync(
            connection,
            sqliteTransaction,
            command.Admission.ContractName,
            cancellationToken);
        if (!string.Equals(
                currentContractDigest,
                command.Admission.ContractDigest,
                StringComparison.Ordinal))
        {
            var supersededCount = await SupersedeStaleCandidatesAsync(
                connection,
                sqliteTransaction,
                command.Admission.ContractName,
                currentContractDigest,
                cancellationToken);
            await transaction.CommitAsync(cancellationToken);
            return new AnalysisCandidateResolutionResult(
                AnalysisCandidateResolutionOutcome.Superseded,
                command.Admission.ExecutableDigest,
                PreviousExecutableDigest: null,
                command.ExpectedCandidateStateVersion + 1,
                supersededCount);
        }

        var newestExecutableDigest = await ReadNewestPendingExecutableDigestAsync(
            connection,
            sqliteTransaction,
            command.Admission.ContractName,
            command.Admission.ContractDigest,
            cancellationToken);
        if (!string.Equals(
                newestExecutableDigest,
                command.Admission.ExecutableDigest,
                StringComparison.Ordinal))
        {
            await transaction.CommitAsync(cancellationToken);
            return Yielded(command.Admission.ExecutableDigest);
        }

        if (command.Action == AnalysisCandidateResolutionAction.Reject)
        {
            var rejected = await TransitionCandidateToInactiveAsync(
                connection,
                sqliteTransaction,
                command.Admission.ExecutableDigest,
                command.ExpectedCandidateStateVersion,
                cancellationToken);
            if (!rejected)
            {
                await transaction.RollbackAsync(cancellationToken);
                return Yielded(command.Admission.ExecutableDigest);
            }

            var supersededOnRejection = await SupersedeOtherCandidatesAsync(
                connection,
                sqliteTransaction,
                command.Admission.ContractName,
                command.Admission.ExecutableDigest,
                cancellationToken);
            await transaction.CommitAsync(cancellationToken);
            return new AnalysisCandidateResolutionResult(
                AnalysisCandidateResolutionOutcome.Rejected,
                command.Admission.ExecutableDigest,
                PreviousExecutableDigest: null,
                command.ExpectedCandidateStateVersion + 1,
                supersededOnRejection);
        }

        var active = await ReadActiveIdentity(
            connection,
            sqliteTransaction,
            command.Admission.ContractDigest,
            cancellationToken);
        if (!LifecycleTokensMatch(active, command.ExpectedActiveExecutable))
        {
            await transaction.CommitAsync(cancellationToken);
            return Yielded(command.Admission.ExecutableDigest);
        }

        if (active is { } previous
            && !await DeactivateExpectedAsync(
                connection,
                sqliteTransaction,
                command.Admission.ContractDigest,
                previous.ExecutableDigest,
                previous.StateVersion,
                cancellationToken))
        {
            await transaction.RollbackAsync(cancellationToken);
            return Yielded(command.Admission.ExecutableDigest);
        }

        var nextStateVersion = (active?.StateVersion
            ?? command.ExpectedCandidateStateVersion) + 1;
        if (!await ActivateExpectedCandidateAsync(
                connection,
                sqliteTransaction,
                command.Admission.ExecutableDigest,
                command.ExpectedCandidateStateVersion,
                nextStateVersion,
                cancellationToken))
        {
            await transaction.RollbackAsync(cancellationToken);
            return Yielded(command.Admission.ExecutableDigest);
        }

        var superseded = await SupersedeOtherCandidatesAsync(
            connection,
            sqliteTransaction,
            command.Admission.ContractName,
            command.Admission.ExecutableDigest,
            cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        return new AnalysisCandidateResolutionResult(
            AnalysisCandidateResolutionOutcome.Activated,
            command.Admission.ExecutableDigest,
            active?.ExecutableDigest,
            nextStateVersion,
            superseded);
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

    private static void ValidateResolutionCommand(
        AnalysisCandidateResolutionCommand command)
    {
        ArgumentNullException.ThrowIfNull(command);
        if (command.ExpectedCandidateStateVersion < 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(command),
                "Expected Candidate state version cannot be negative.");
        }

        if (command.ExpectedActiveExecutable?.StateVersion < 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(command),
                "Expected active executable state version cannot be negative.");
        }

        if (!Enum.IsDefined(command.Action))
        {
            throw new ArgumentOutOfRangeException(
                nameof(command),
                "Candidate resolution action is not supported.");
        }

        var admission = command.Admission;
        if (string.IsNullOrWhiteSpace(admission.WorkspaceId)
            || string.IsNullOrWhiteSpace(admission.CycleId)
            || string.IsNullOrWhiteSpace(admission.ContractName)
            || string.IsNullOrWhiteSpace(admission.ContractDigest)
            || string.IsNullOrWhiteSpace(admission.ExecutableDigest))
        {
            throw new ArgumentException(
                "Candidate resolution requires complete admission identity.",
                nameof(command));
        }
    }

    private static async Task<(
        ExecutableLifecycleState State,
        long StateVersion,
        DateTimeOffset CreatedAt)?> ReadResolutionCandidateAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        AnalysisCandidateAdmission admission,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT executable.lifecycle_state,
                   executable.state_version,
                   candidate.created_at
            FROM analysis_candidate_admissions AS candidate
            JOIN decision_executables AS executable
              ON executable.executable_digest = candidate.executable_digest
            WHERE candidate.workspace_id = $workspaceId
              AND candidate.cycle_id = $cycleId
              AND candidate.contract_name = $contractName
              AND candidate.contract_digest = $contractDigest
              AND candidate.executable_digest = $executableDigest;
            """;
        command.Parameters.AddWithValue("$workspaceId", admission.WorkspaceId);
        command.Parameters.AddWithValue("$cycleId", admission.CycleId);
        command.Parameters.AddWithValue("$contractName", admission.ContractName);
        command.Parameters.AddWithValue("$contractDigest", admission.ContractDigest);
        command.Parameters.AddWithValue("$executableDigest", admission.ExecutableDigest);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        if (!await reader.ReadAsync(cancellationToken))
        {
            return null;
        }

        var state = reader.GetString(0) switch
        {
            "candidate" => ExecutableLifecycleState.Candidate,
            "active" => ExecutableLifecycleState.Active,
            "inactive" => ExecutableLifecycleState.Inactive,
            var invalid => throw new InvalidDataException(
                $"Unknown executable lifecycle state '{invalid}'.")
        };
        return (state, reader.GetInt64(1), ParseTime(reader.GetString(2)));
    }

    private static async Task<string?> ReadCurrentContractDigestAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractName,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT contract_digest
            FROM decision_contract_current
            WHERE contract_name = $contractName;
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        return await command.ExecuteScalarAsync(cancellationToken) as string;
    }

    private static async Task<string?> ReadNewestPendingExecutableDigestAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            SELECT candidate.executable_digest
            FROM analysis_candidate_admissions AS candidate
            JOIN decision_executables AS executable
              ON executable.executable_digest = candidate.executable_digest
            WHERE candidate.contract_name = $contractName
              AND candidate.contract_digest = $contractDigest
              AND executable.lifecycle_state = 'candidate'
            ORDER BY candidate.created_at DESC,
                     candidate.executable_digest DESC,
                     candidate.workspace_id DESC,
                     candidate.cycle_id DESC
            LIMIT 1;
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        command.Parameters.AddWithValue("$contractDigest", contractDigest);
        return await command.ExecuteScalarAsync(cancellationToken) as string;
    }

    private static async Task<int> SupersedeStaleCandidatesAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractName,
        string? currentContractDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'inactive',
                state_version = state_version + 1
            WHERE lifecycle_state = 'candidate'
              AND EXISTS (
                  SELECT 1
                  FROM analysis_candidate_admissions AS candidate
                  WHERE candidate.executable_digest =
                            decision_executables.executable_digest
                    AND candidate.contract_name = $contractName
                    AND (
                        $currentContractDigest IS NULL
                        OR candidate.contract_digest <> $currentContractDigest
                    )
              );
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        command.Parameters.AddWithValue(
            "$currentContractDigest",
            currentContractDigest is null ? DBNull.Value : currentContractDigest);
        return await command.ExecuteNonQueryAsync(cancellationToken);
    }

    private static async Task<bool> TransitionCandidateToInactiveAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string executableDigest,
        long expectedStateVersion,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'inactive',
                state_version = state_version + 1
            WHERE executable_digest = $executableDigest
              AND lifecycle_state = 'candidate'
              AND state_version = $expectedStateVersion;
            """;
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        command.Parameters.AddWithValue("$expectedStateVersion", expectedStateVersion);
        return await command.ExecuteNonQueryAsync(cancellationToken) == 1;
    }

    private static bool LifecycleTokensMatch(
        (string ExecutableDigest, long StateVersion)? actual,
        ExecutableLifecycleToken? expected) =>
        actual is null
            ? expected is null
            : expected is not null
              && string.Equals(
                  actual.Value.ExecutableDigest,
                  expected.ExecutableDigest,
                  StringComparison.Ordinal)
              && actual.Value.StateVersion == expected.StateVersion;

    private static async Task<bool> DeactivateExpectedAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractDigest,
        string executableDigest,
        long expectedStateVersion,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'inactive',
                state_version = state_version + 1
            WHERE contract_digest = $contractDigest
              AND executable_digest = $executableDigest
              AND lifecycle_state = 'active'
              AND state_version = $expectedStateVersion;
            """;
        command.Parameters.AddWithValue("$contractDigest", contractDigest);
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        command.Parameters.AddWithValue("$expectedStateVersion", expectedStateVersion);
        return await command.ExecuteNonQueryAsync(cancellationToken) == 1;
    }

    private async Task<bool> ActivateExpectedCandidateAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string executableDigest,
        long expectedStateVersion,
        long nextStateVersion,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'active',
                state_version = $nextStateVersion,
                activated_at = $activatedAt
            WHERE executable_digest = $executableDigest
              AND lifecycle_state = 'candidate'
              AND state_version = $expectedStateVersion;
            """;
        command.Parameters.AddWithValue("$executableDigest", executableDigest);
        command.Parameters.AddWithValue("$expectedStateVersion", expectedStateVersion);
        command.Parameters.AddWithValue("$nextStateVersion", nextStateVersion);
        command.Parameters.AddWithValue("$activatedAt", FormatTime(_timeProvider.GetUtcNow()));
        return await command.ExecuteNonQueryAsync(cancellationToken) == 1;
    }

    private static async Task<int> SupersedeOtherCandidatesAsync(
        SqliteConnection connection,
        SqliteTransaction transaction,
        string contractName,
        string selectedExecutableDigest,
        CancellationToken cancellationToken)
    {
        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            UPDATE decision_executables
            SET lifecycle_state = 'inactive',
                state_version = state_version + 1
            WHERE executable_digest <> $selectedExecutableDigest
              AND lifecycle_state = 'candidate'
              AND EXISTS (
                  SELECT 1
                  FROM analysis_candidate_admissions AS candidate
                  WHERE candidate.executable_digest =
                            decision_executables.executable_digest
                    AND candidate.contract_name = $contractName
              );
            """;
        command.Parameters.AddWithValue("$selectedExecutableDigest", selectedExecutableDigest);
        command.Parameters.AddWithValue("$contractName", contractName);
        return await command.ExecuteNonQueryAsync(cancellationToken);
    }

    private static AnalysisCandidateResolutionResult Yielded(
        string executableDigest) =>
        new(
            AnalysisCandidateResolutionOutcome.Yielded,
            executableDigest,
            PreviousExecutableDigest: null,
            StateVersion: null,
            SupersededCandidateCount: 0);

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
                StringComparison.Ordinal)
            || admission.CreatedAt != executable.CreatedAt)
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
