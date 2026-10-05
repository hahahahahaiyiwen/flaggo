using System.Globalization;
using System.Text;
using Flaggo.Contract;
using Microsoft.Data.Sqlite;

namespace Flaggo.ContractStore;

public sealed class SqliteContractVersionStore : IContractVersionStore
{
    private const string ComponentName = "contract-store";
    private const int SchemaVersion = 3;
    private readonly string _connectionString;
    private readonly IExpressionCanonicalizer? _expressionCanonicalizer;

    public SqliteContractVersionStore(
        string connectionString,
        IExpressionCanonicalizer? expressionCanonicalizer = null)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(connectionString);
        _connectionString = connectionString;
        _expressionCanonicalizer = expressionCanonicalizer;
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

            CREATE TABLE IF NOT EXISTS decision_contract_resources (
                contract_name TEXT PRIMARY KEY,
                tenant TEXT NOT NULL,
                application TEXT NOT NULL,
                environment TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS decision_contract_versions (
                contract_name TEXT NOT NULL,
                contract_digest TEXT NOT NULL,
                accepted_at TEXT NOT NULL,
                contract_json BLOB NOT NULL,
                PRIMARY KEY(contract_name, contract_digest),
                FOREIGN KEY(contract_name)
                    REFERENCES decision_contract_resources(contract_name)
            );

            CREATE INDEX IF NOT EXISTS ix_contract_versions_history
            ON decision_contract_versions(
                contract_name,
                accepted_at DESC,
                contract_digest DESC
            );

            CREATE TABLE IF NOT EXISTS decision_contract_current (
                contract_name TEXT NOT NULL,
                contract_digest TEXT NOT NULL,
                PRIMARY KEY(contract_name),
                FOREIGN KEY(contract_name, contract_digest)
                    REFERENCES decision_contract_versions(
                        contract_name,
                        contract_digest
                    )
            );
            """;
        command.Parameters.AddWithValue("$component", ComponentName);
        command.Parameters.AddWithValue("$version", SchemaVersion);
        await command.ExecuteNonQueryAsync(cancellationToken);
    }

    public async Task<ContractStoreWriteResult> PutAsync(
        AcceptedContractVersion version,
        CancellationToken cancellationToken = default)
    {
        var storedContract = ValidateVersion(version);
        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = connection.BeginTransaction(
            System.Data.IsolationLevel.Serializable,
            deferred: false);
        await using var claim = connection.CreateCommand();
        claim.Transaction = transaction;
        claim.CommandText =
            """
            INSERT INTO decision_contract_resources(
                contract_name,
                tenant,
                application,
                environment
            )
            VALUES ($contractName, $tenant, $application, $environment)
            ON CONFLICT(contract_name) DO NOTHING;
            """;
        claim.Parameters.AddWithValue("$contractName", version.Contract.Name);
        AddScope(claim, version.Contract.Authority);
        await claim.ExecuteNonQueryAsync(cancellationToken);

        await using var owner = connection.CreateCommand();
        owner.Transaction = transaction;
        owner.CommandText =
            """
            SELECT tenant, application, environment
            FROM decision_contract_resources
            WHERE contract_name = $contractName;
            """;
        owner.Parameters.AddWithValue("$contractName", version.Contract.Name);
        await using (var reader = await owner.ExecuteReaderAsync(cancellationToken))
        {
            if (!await reader.ReadAsync(cancellationToken))
            {
                throw new InvalidOperationException("Contract name ownership was not persisted.");
            }

            var existingAuthority = new AuthorityScope(
                reader.GetString(0),
                reader.GetString(1),
                reader.GetString(2));
            if (existingAuthority != version.Contract.Authority)
            {
                throw new ContractNameAuthorityConflictException(
                    version.Contract.Name,
                    existingAuthority,
                    version.Contract.Authority);
            }
        }

        await using var command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            """
            INSERT INTO decision_contract_versions(
                contract_name,
                contract_digest,
                accepted_at,
                contract_json
            )
            VALUES (
                $contractName,
                $contractDigest,
                $acceptedAt,
                $contractJson
            )
            ON CONFLICT(contract_name, contract_digest)
            DO NOTHING;
            """;
        command.Parameters.AddWithValue("$contractName", version.Contract.Name);
        command.Parameters.AddWithValue("$contractDigest", version.ContractDigest);
        command.Parameters.AddWithValue("$acceptedAt", FormatTime(version.AcceptedAt));
        command.Parameters.Add("$contractJson", SqliteType.Blob).Value =
            StrictJson.SerializeToUtf8Bytes(storedContract);
        var rows = await command.ExecuteNonQueryAsync(cancellationToken);
        await transaction.CommitAsync(cancellationToken);
        return rows == 1 ? ContractStoreWriteResult.Created : ContractStoreWriteResult.Existing;
    }

    public async Task<AcceptedContractVersion?> GetAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT accepted_at, contract_json
            FROM decision_contract_versions
            WHERE contract_name = $contractName
              AND contract_digest = $contractDigest;
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        command.Parameters.AddWithValue("$contractDigest", contractDigest);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        return await reader.ReadAsync(cancellationToken)
            ? ReadVersion(reader, contractName, contractDigest)
            : null;
    }

    public async Task<AcceptedContractVersion?> GetCurrentAsync(
        string contractName,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT versions.contract_digest, versions.accepted_at, versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
              ON versions.contract_name = current.contract_name
             AND versions.contract_digest = current.contract_digest
            WHERE current.contract_name = $contractName;
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        return await reader.ReadAsync(cancellationToken)
            ? ReadVersion(
                reader,
                contractName,
                reader.GetString(0),
                digestColumnOffset: 1)
            : null;
    }

    public async Task<IReadOnlyList<AcceptedContractVersion>> ListAllCurrentAsync(
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT resources.tenant,
                   resources.application,
                   resources.environment,
                   versions.contract_name,
                   versions.contract_digest,
                   versions.accepted_at,
                   versions.contract_json
            FROM decision_contract_current AS current
            JOIN decision_contract_versions AS versions
              ON versions.contract_name = current.contract_name
             AND versions.contract_digest = current.contract_digest
            JOIN decision_contract_resources AS resources
              ON resources.contract_name = versions.contract_name
            ORDER BY resources.tenant ASC,
                     resources.application ASC,
                     resources.environment ASC,
                     versions.contract_name ASC,
                     versions.contract_digest ASC;
            """;
        var versions = new List<AcceptedContractVersion>();
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            versions.Add(ReadVersion(
                reader,
                reader.GetString(3),
                reader.GetString(4),
                digestColumnOffset: 5));
        }

        return versions;
    }

    public async Task<ContractVersionPage> ListAsync(
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default)
    {
        if (pageSize is < 1 or > 200)
        {
            throw new ArgumentOutOfRangeException(nameof(pageSize), "Page size must be between 1 and 200.");
        }

        (DateTimeOffset AcceptedAt, string ContractDigest)? boundary =
            cursor is null ? null : DecodeCursor(cursor);
        await using var connection = await OpenAsync(cancellationToken);
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            SELECT contract_digest, accepted_at, contract_json
            FROM decision_contract_versions
            WHERE contract_name = $contractName
              AND (
                    $cursorAcceptedAt IS NULL
                    OR accepted_at < $cursorAcceptedAt
                    OR (accepted_at = $cursorAcceptedAt AND contract_digest < $cursorDigest)
                  )
            ORDER BY accepted_at DESC, contract_digest DESC
            LIMIT $limit;
            """;
        command.Parameters.AddWithValue("$contractName", contractName);
        command.Parameters.AddWithValue(
            "$cursorAcceptedAt",
            boundary is null ? DBNull.Value : FormatTime(boundary.Value.AcceptedAt));
        command.Parameters.AddWithValue(
            "$cursorDigest",
            boundary is null ? DBNull.Value : boundary.Value.ContractDigest);
        command.Parameters.AddWithValue("$limit", pageSize + 1);

        var versions = new List<AcceptedContractVersion>(pageSize + 1);
        await using var reader = await command.ExecuteReaderAsync(cancellationToken);
        while (await reader.ReadAsync(cancellationToken))
        {
            versions.Add(ReadVersion(
                reader,
                contractName,
                reader.GetString(0),
                digestColumnOffset: 1));
        }

        string? nextCursor = null;
        if (versions.Count > pageSize)
        {
            versions.RemoveAt(versions.Count - 1);
            var last = versions[^1];
            nextCursor = EncodeCursor(last.AcceptedAt, last.ContractDigest);
        }

        return new ContractVersionPage(versions, nextCursor);
    }

    public async Task SetCurrentAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default)
    {
        await using var connection = await OpenAsync(cancellationToken);
        await using var transaction = await connection.BeginTransactionAsync(cancellationToken);
        await using var exists = connection.CreateCommand();
        exists.Transaction = (SqliteTransaction)transaction;
        exists.CommandText =
            """
            SELECT 1
            FROM decision_contract_versions
            WHERE contract_name = $contractName
              AND contract_digest = $contractDigest;
            """;
        exists.Parameters.AddWithValue("$contractName", contractName);
        exists.Parameters.AddWithValue("$contractDigest", contractDigest);
        if (await exists.ExecuteScalarAsync(cancellationToken) is null)
        {
            throw new ContractVersionNotFoundException(contractName, contractDigest);
        }

        await using var update = connection.CreateCommand();
        update.Transaction = (SqliteTransaction)transaction;
        update.CommandText =
            """
            INSERT INTO decision_contract_current(
                contract_name,
                contract_digest
            )
            VALUES (
                $contractName,
                $contractDigest
            )
            ON CONFLICT(contract_name)
            DO UPDATE SET contract_digest = excluded.contract_digest;
            """;
        update.Parameters.AddWithValue("$contractName", contractName);
        update.Parameters.AddWithValue("$contractDigest", contractDigest);
        await update.ExecuteNonQueryAsync(cancellationToken);
        await transaction.CommitAsync(cancellationToken);
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
                SELECT contract_name, tenant, application, environment
                FROM decision_contract_resources
                LIMIT 0;
                """,
                cancellationToken);
            await ProbeTableAsync(
                connection,
                """
                SELECT contract_name, contract_digest,
                       accepted_at, contract_json
                FROM decision_contract_versions
                LIMIT 0;
                """,
                cancellationToken);
            await ProbeTableAsync(
                connection,
                """
                SELECT contract_name, contract_digest
                FROM decision_contract_current
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

    private DecisionContract ValidateVersion(AcceptedContractVersion version)
    {
        ValidateScope(version.Contract.Authority);

        if (!string.Equals(version.Contract.Name, version.Contract.Name.Trim(), StringComparison.Ordinal))
        {
            throw new ArgumentException("Contract name cannot contain surrounding whitespace.", nameof(version));
        }

        var digest = ContractDigests.ComputeContractDigest(
            version.Contract,
            _expressionCanonicalizer);
        if (!string.Equals(digest, version.ContractDigest, StringComparison.Ordinal))
        {
            throw new ArgumentException(
                $"Contract digest mismatch: expected {digest}, received {version.ContractDigest}.",
                nameof(version));
        }

        return _expressionCanonicalizer is null
            ? version.Contract
            : ContractDigests.CanonicalizeExpressions(
                version.Contract,
                _expressionCanonicalizer);
    }

    private static void ValidateScope(AuthorityScope scope)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Tenant);
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Application);
        ArgumentException.ThrowIfNullOrWhiteSpace(scope.Environment);
    }

    private static void AddScope(SqliteCommand command, AuthorityScope scope)
    {
        command.Parameters.AddWithValue("$tenant", scope.Tenant);
        command.Parameters.AddWithValue("$application", scope.Application);
        command.Parameters.AddWithValue("$environment", scope.Environment);
    }

    private AcceptedContractVersion ReadVersion(
        SqliteDataReader reader,
        string contractName,
        string contractDigest,
        int digestColumnOffset = 0)
    {
        var acceptedAt = DateTimeOffset.ParseExact(
            reader.GetString(digestColumnOffset),
            "O",
            CultureInfo.InvariantCulture,
            DateTimeStyles.RoundtripKind);
        var contract = StrictJson.Deserialize<DecisionContract>(
            (byte[])reader.GetValue(digestColumnOffset + 1));
        if (!string.Equals(contract.Name, contractName, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                $"Stored contract name '{contract.Name}' does not match key '{contractName}'.");
        }

        ValidateScope(contract.Authority);

        var computedDigest = ContractDigests.ComputeContractDigest(contract);
        if (!string.Equals(computedDigest, contractDigest, StringComparison.Ordinal))
        {
            throw new InvalidDataException(
                $"Stored contract digest '{contractDigest}' does not match payload '{computedDigest}'.");
        }

        return new AcceptedContractVersion(contractDigest, acceptedAt, contract);
    }

    private static string FormatTime(DateTimeOffset value) =>
        value.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture);

    private static string EncodeCursor(DateTimeOffset acceptedAt, string contractDigest)
    {
        var payload = Encoding.UTF8.GetBytes($"{FormatTime(acceptedAt)}\n{contractDigest}");
        return Convert.ToBase64String(payload)
            .TrimEnd('=')
            .Replace('+', '-')
            .Replace('/', '_');
    }

    private static (DateTimeOffset AcceptedAt, string ContractDigest) DecodeCursor(string cursor)
    {
        try
        {
            var padded = cursor.Replace('-', '+').Replace('_', '/');
            padded = padded.PadRight((padded.Length + 3) / 4 * 4, '=');
            var payload = Encoding.UTF8.GetString(Convert.FromBase64String(padded));
            var separator = payload.IndexOf('\n');
            if (separator <= 0)
            {
                throw new FormatException();
            }

            var acceptedAt = DateTimeOffset.ParseExact(
                payload[..separator],
                "O",
                CultureInfo.InvariantCulture,
                DateTimeStyles.RoundtripKind);
            var digest = payload[(separator + 1)..];
            if (!ContractDigests.IsSha256Digest(digest))
            {
                throw new FormatException();
            }

            return (acceptedAt, digest);
        }
        catch (Exception exception) when (
            exception is FormatException or ArgumentException)
        {
            throw new ArgumentException("Invalid contract-version cursor.", nameof(cursor), exception);
        }
    }
}
