using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

namespace Flaggo.Core.Tests;

public sealed class StoreTests
{
    private static readonly DecisionScope Scope = new("checkout", "production");

    [Fact]
    public async Task ContractVersionsAreImmutableScopedAndPersistent()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteContractVersionStore(database.ConnectionString);
        await store.InitializeAsync();
        var contract = CreateContract(defaultValue: 100);
        var digest = ContractDigests.ComputeContractDigest(contract);
        var acceptedAt = new DateTimeOffset(2026, 3, 1, 10, 0, 0, TimeSpan.Zero);
        var version = new AcceptedContractVersion(Scope, digest, acceptedAt, contract);

        Assert.Equal(ContractStoreWriteResult.Created, await store.PutAsync(version));
        Assert.Equal(
            ContractStoreWriteResult.Existing,
            await store.PutAsync(version with { AcceptedAt = acceptedAt.AddHours(1) }));

        var persisted = await new SqliteContractVersionStore(database.ConnectionString)
            .GetAsync(Scope, contract.Name, digest);

        Assert.NotNull(persisted);
        Assert.Equal(acceptedAt, persisted.AcceptedAt);
        Assert.Equal(100, persisted.Contract.Result.Default.GetInt32());
        Assert.Null(await store.GetAsync(
            new DecisionScope("checkout", "staging"),
            contract.Name,
            digest));

        var changedContract = CreateContract(defaultValue: 200);
        await Assert.ThrowsAsync<ArgumentException>(() => store.PutAsync(
            version with { Contract = changedContract }));
        Assert.Equal(100, (await store.GetAsync(Scope, contract.Name, digest))!
            .Contract.Result.Default.GetInt32());
    }

    [Fact]
    public async Task ContractCurrentAndHistoryUseStableCursorPagination()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteContractVersionStore(database.ConnectionString);
        await store.InitializeAsync();
        var acceptedAt = new DateTimeOffset(2026, 3, 2, 10, 0, 0, TimeSpan.Zero);
        var versions = new[]
        {
            CreateAcceptedVersion(100, acceptedAt),
            CreateAcceptedVersion(200, acceptedAt.AddMinutes(1)),
            CreateAcceptedVersion(300, acceptedAt.AddMinutes(1))
        };

        foreach (var version in versions)
        {
            await store.PutAsync(version);
        }

        await store.SetCurrentAsync(Scope, versions[1].Contract.Name, versions[1].ContractDigest);
        var current = await store.GetCurrentAsync(Scope, versions[1].Contract.Name);

        Assert.NotNull(current);
        Assert.Equal(versions[1].ContractDigest, current.ContractDigest);

        var first = await store.ListAsync(Scope, versions[0].Contract.Name, 2, cursor: null);
        Assert.Equal(2, first.Versions.Count);
        Assert.NotNull(first.NextCursor);
        var second = await store.ListAsync(
            Scope,
            versions[0].Contract.Name,
            2,
            first.NextCursor);
        Assert.Single(second.Versions);
        Assert.Null(second.NextCursor);

        var expected = versions
            .OrderByDescending(version => version.AcceptedAt)
            .ThenByDescending(
                version => version.ContractDigest,
                StringComparer.Ordinal)
            .Select(version => version.ContractDigest);
        Assert.Equal(
            expected,
            first.Versions.Concat(second.Versions).Select(version => version.ContractDigest));

        await Assert.ThrowsAsync<ArgumentException>(
            () => store.ListAsync(Scope, versions[0].Contract.Name, 2, "not-a-cursor"));
        await Assert.ThrowsAsync<ContractVersionNotFoundException>(
            () => store.SetCurrentAsync(
                Scope,
                versions[0].Contract.Name,
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"));
    }

    [Fact]
    public async Task ExecutableCandidatesAreImmutableScopedAndPersistent()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteExecutableStore(database.ConnectionString);
        await store.InitializeAsync();
        var contract = CreateContract(100);
        var createdAt = new DateTimeOffset(2026, 3, 3, 10, 0, 0, TimeSpan.Zero);
        var candidate = CreateCandidate(contract, variant: 1, createdAt);

        Assert.Equal(
            ExecutableStoreWriteResult.Created,
            await store.PutCandidateAsync(candidate));
        Assert.Equal(
            ExecutableStoreWriteResult.Existing,
            await store.PutCandidateAsync(candidate with
            {
                CreatedAt = createdAt.AddHours(1),
                Provenance = JsonSerializer.SerializeToElement(new { source = "replacement" })
            }));

        var persisted = await new SqliteExecutableStore(database.ConnectionString)
            .GetAsync(Scope, candidate.ExecutableDigest);

        Assert.NotNull(persisted);
        Assert.Equal(ExecutableLifecycleState.Candidate, persisted.State);
        Assert.Equal(0, persisted.StateVersion);
        Assert.Equal(createdAt, persisted.CreatedAt);
        Assert.Equal("test", persisted.Provenance!.Value.GetProperty("source").GetString());
        Assert.Null(await store.GetAsync(
            new DecisionScope("checkout", "staging"),
            candidate.ExecutableDigest));

        await Assert.ThrowsAsync<ArgumentException>(() => store.PutCandidateAsync(
            candidate with
            {
                ExecutableDigest =
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            }));
    }

    [Fact]
    public async Task ActivationAtomicallyReplacesOnlyTheMatchingContract()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteExecutableStore(database.ConnectionString);
        await store.InitializeAsync();
        var firstContract = CreateContract(100);
        var secondContract = CreateContract(200);
        var firstContractDigest = ContractDigests.ComputeContractDigest(firstContract);
        var secondContractDigest = ContractDigests.ComputeContractDigest(secondContract);
        var first = CreateCandidate(firstContract, variant: 1);
        var replacement = CreateCandidate(firstContract, variant: 2);
        var otherContract = CreateCandidate(secondContract, variant: 3);
        await store.PutCandidateAsync(first);
        await store.PutCandidateAsync(replacement);
        await store.PutCandidateAsync(otherContract);

        var initial = await store.ActivateIfNoneAsync(
            Scope,
            firstContractDigest,
            first.ExecutableDigest);
        var unchanged = await store.ActivateAsync(
            Scope,
            firstContractDigest,
            first.ExecutableDigest,
            first.ExecutableDigest);

        Assert.True(initial.Changed);
        Assert.Equal(1, initial.StateVersion);
        Assert.False(unchanged.Changed);
        Assert.Equal(1, unchanged.StateVersion);

        await Assert.ThrowsAsync<ActivationConflictException>(
            () => store.ActivateIfNoneAsync(
                Scope,
                firstContractDigest,
                replacement.ExecutableDigest));
        Assert.Equal(
            first.ExecutableDigest,
            (await store.GetActiveAsync(Scope, firstContractDigest))!.ExecutableDigest);

        await Assert.ThrowsAsync<ActivationConflictException>(() => store.ActivateAsync(
            Scope,
            firstContractDigest,
            replacement.ExecutableDigest,
            expectedActiveExecutableDigest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"));
        Assert.Equal(
            first.ExecutableDigest,
            (await store.GetActiveAsync(Scope, firstContractDigest))!.ExecutableDigest);

        var replaced = await store.ActivateAsync(
            Scope,
            firstContractDigest,
            replacement.ExecutableDigest,
            first.ExecutableDigest);
        Assert.True(replaced.Changed);
        Assert.Equal(first.ExecutableDigest, replaced.PreviousExecutableDigest);
        Assert.Equal(2, replaced.StateVersion);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(Scope, first.ExecutableDigest))!.State);
        Assert.Equal(
            ExecutableLifecycleState.Active,
            (await store.GetAsync(Scope, replacement.ExecutableDigest))!.State);

        await store.ActivateAsync(Scope, secondContractDigest, otherContract.ExecutableDigest);
        Assert.Equal(
            replacement.ExecutableDigest,
            (await store.GetActiveAsync(Scope, firstContractDigest))!.ExecutableDigest);
        Assert.Equal(
            otherContract.ExecutableDigest,
            (await store.GetActiveAsync(Scope, secondContractDigest))!.ExecutableDigest);

        var reactivated = await store.ActivateAsync(
            Scope,
            firstContractDigest,
            first.ExecutableDigest,
            replacement.ExecutableDigest);
        Assert.Equal(3, reactivated.StateVersion);
        Assert.Equal(
            3,
            (await store.GetAsync(Scope, replacement.ExecutableDigest))!.StateVersion);
    }

    [Fact]
    public async Task ConcurrentExpectedActivationHasOneWinner()
    {
        using var database = new TemporaryDatabase();
        var setupStore = new SqliteExecutableStore(database.ConnectionString);
        await setupStore.InitializeAsync();
        var contract = CreateContract(100);
        var contractDigest = ContractDigests.ComputeContractDigest(contract);
        var initial = CreateCandidate(contract, variant: 1);
        var left = CreateCandidate(contract, variant: 2);
        var right = CreateCandidate(contract, variant: 3);
        await setupStore.PutCandidateAsync(initial);
        await setupStore.PutCandidateAsync(left);
        await setupStore.PutCandidateAsync(right);
        await setupStore.ActivateAsync(Scope, contractDigest, initial.ExecutableDigest);

        var results = await Task.WhenAll(
            TryActivateAsync(
                new SqliteExecutableStore(database.ConnectionString),
                contractDigest,
                left.ExecutableDigest,
                initial.ExecutableDigest),
            TryActivateAsync(
                new SqliteExecutableStore(database.ConnectionString),
                contractDigest,
                right.ExecutableDigest,
                initial.ExecutableDigest));

        var success = Assert.Single(results, result => result.Result is not null);
        var failure = Assert.Single(results, result => result.Error is not null);
        Assert.IsType<ActivationConflictException>(failure.Error);
        Assert.Equal(
            success.Result!.ExecutableDigest,
            (await setupStore.GetActiveAsync(Scope, contractDigest))!.ExecutableDigest);
    }

    [Fact]
    public async Task ContractAndExecutableStoresCanShareOneDatabase()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);

        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();

        Assert.True(await contractStore.IsAvailableAsync());
        Assert.True(await executableStore.IsAvailableAsync());
    }

    [Fact]
    public async Task AvailabilityRejectsUnknownOwnedSchemaVersions()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();

        await ExecuteSqlAsync(
            database.ConnectionString,
            "UPDATE flaggo_schema_versions SET version = 2 "
            + "WHERE component = 'contract-store';");
        Assert.False(await contractStore.IsAvailableAsync());
        Assert.True(await executableStore.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "UPDATE flaggo_schema_versions SET version = 1 "
            + "WHERE component = 'contract-store'; "
            + "UPDATE flaggo_schema_versions SET version = 2 "
            + "WHERE component = 'executable-store';");
        Assert.True(await contractStore.IsAvailableAsync());
        Assert.False(await executableStore.IsAvailableAsync());
    }

    [Fact]
    public async Task AvailabilityRejectsMissingOwnedTables()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();

        await ExecuteSqlAsync(
            database.ConnectionString,
            "DROP TABLE decision_contract_current;");
        Assert.False(await contractStore.IsAvailableAsync());
        Assert.True(await executableStore.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "DROP TABLE decision_executables;");
        Assert.False(await executableStore.IsAvailableAsync());
    }

    private static AcceptedContractVersion CreateAcceptedVersion(
        int defaultValue,
        DateTimeOffset acceptedAt)
    {
        var contract = CreateContract(defaultValue);
        return new AcceptedContractVersion(
            Scope,
            ContractDigests.ComputeContractDigest(contract),
            acceptedAt,
            contract);
    }

    private static DecisionContract CreateContract(int defaultValue) =>
        new()
        {
            Name = "checkout.delay",
            ExpressionSyntax = "flaggo.cel/v1",
            Attributes = [],
            Result = new ContractResult
            {
                Schema = new ValueSchema { Type = "integer" },
                Default = JsonSerializer.SerializeToElement(defaultValue)
            }
        };

    private static StoredExecutable CreateCandidate(
        DecisionContract contract,
        int variant,
        DateTimeOffset? createdAt = null)
    {
        var expressionCompiler = new FlaggoExpressionCompiler();
        var executable = new DecisionExecutable
        {
            ContractDigest = ContractDigests.ComputeContractDigest(
                contract,
                expressionCompiler),
            Rules =
            [
                new ExecutableRule
                {
                    Name = $"rule-{variant}",
                    When = new ExpressionWhen("true"),
                    Return = new LiteralReturn(JsonSerializer.SerializeToElement(variant))
                }
            ]
        };
        var compilation = new FlaggoExecutableCompiler(expressionCompiler)
            .Compile(contract, executable);
        return new StoredExecutable(
            Scope,
            compilation.ExecutableDigest,
            compilation.Executable,
            compilation.CheckedExecutable,
            ExecutableLifecycleState.Candidate,
            StateVersion: 0,
            CreatedAt: createdAt
                ?? new DateTimeOffset(2026, 3, 3, 10, variant, 0, TimeSpan.Zero),
            ActivatedAt: null,
            Provenance: JsonSerializer.SerializeToElement(new { source = "test" }));
    }

    private static async Task<(ActivationResult? Result, Exception? Error)> TryActivateAsync(
        IExecutableStore store,
        string contractDigest,
        string executableDigest,
        string expectedExecutableDigest)
    {
        try
        {
            return (
                await store.ActivateAsync(
                    Scope,
                    contractDigest,
                    executableDigest,
                    expectedExecutableDigest),
                null);
        }
        catch (Exception exception)
        {
            return (null, exception);
        }
    }

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
            Path.Combine(Path.GetTempPath(), $"flaggo-{Guid.NewGuid():N}.db");

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
