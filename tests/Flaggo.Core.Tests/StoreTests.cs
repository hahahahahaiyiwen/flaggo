using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;
using Microsoft.Data.Sqlite;

namespace Flaggo.Core.Tests;

public sealed class StoreTests
{
    private static readonly AuthorityScope Scope = new("local", "checkout", "production");

    [Fact]
    public async Task ContractVersionsAreImmutableAndPersistent()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteContractVersionStore(database.ConnectionString);
        await store.InitializeAsync();
        var contract = CreateContract(defaultValue: 100);
        var digest = ContractDigests.ComputeContractDigest(contract);
        var acceptedAt = new DateTimeOffset(2026, 3, 1, 10, 0, 0, TimeSpan.Zero);
        var version = new AcceptedContractVersion(digest, acceptedAt, contract);

        Assert.Equal(ContractStoreWriteResult.Created, await store.PutAsync(version));
        Assert.Equal(
            ContractStoreWriteResult.Existing,
            await store.PutAsync(version with { AcceptedAt = acceptedAt.AddHours(1) }));

        var persisted = await new SqliteContractVersionStore(database.ConnectionString)
            .GetAsync(contract.Name, digest);

        Assert.NotNull(persisted);
        Assert.Equal(acceptedAt, persisted.AcceptedAt);
        Assert.Equal(100, persisted.Contract.Result.Default.GetInt32());
        var changedContract = CreateContract(defaultValue: 200);
        await Assert.ThrowsAsync<ArgumentException>(() => store.PutAsync(
            version with { Contract = changedContract }));
        Assert.Equal(100, (await store.GetAsync(contract.Name, digest))!
            .Contract.Result.Default.GetInt32());
    }

    [Fact]
    public async Task ContractNameCannotMoveToAnotherAuthority()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteContractVersionStore(database.ConnectionString);
        await store.InitializeAsync();
        var firstContract = CreateContract(defaultValue: 100);
        var secondScope = new AuthorityScope("acme", "checkout", "production");
        var secondContract = firstContract with { Authority = secondScope };
        var first = new AcceptedContractVersion(
            ContractDigests.ComputeContractDigest(firstContract),
            new DateTimeOffset(2026, 3, 1, 10, 0, 0, TimeSpan.Zero),
            firstContract);
        var second = new AcceptedContractVersion(
            ContractDigests.ComputeContractDigest(secondContract),
            first.AcceptedAt,
            secondContract);

        Assert.NotEqual(first.ContractDigest, second.ContractDigest);
        await store.PutAsync(first);
        var conflict = await Assert.ThrowsAsync<ContractNameAuthorityConflictException>(
            () => store.PutAsync(second));
        Assert.Equal(first.Contract.Name, conflict.ContractName);
        Assert.Equal(Scope, conflict.ExistingAuthority);
        Assert.Equal(secondScope, conflict.RequestedAuthority);
    }

    [Fact]
    public async Task ConcurrentFirstWritesEstablishOneContractNameOwner()
    {
        using var database = new TemporaryDatabase();
        var firstStore = new SqliteContractVersionStore(database.ConnectionString);
        var secondStore = new SqliteContractVersionStore(database.ConnectionString);
        await firstStore.InitializeAsync();
        await secondStore.InitializeAsync();
        var firstContract = CreateContract(defaultValue: 100);
        var secondContract = firstContract with
        {
            Authority = new AuthorityScope("acme", "checkout", "production")
        };
        var acceptedAt = new DateTimeOffset(2026, 3, 1, 10, 0, 0, TimeSpan.Zero);
        var first = new AcceptedContractVersion(
            ContractDigests.ComputeContractDigest(firstContract),
            acceptedAt,
            firstContract);
        var second = new AcceptedContractVersion(
            ContractDigests.ComputeContractDigest(secondContract),
            acceptedAt,
            secondContract);

        static async Task<object> PutAsync(
            IContractVersionStore store,
            AcceptedContractVersion version)
        {
            try
            {
                return await store.PutAsync(version);
            }
            catch (ContractNameAuthorityConflictException conflict)
            {
                return conflict;
            }
        }

        var results = await Task.WhenAll(
            PutAsync(firstStore, first),
            PutAsync(secondStore, second));

        Assert.Single(results, result => result is ContractStoreWriteResult.Created);
        Assert.Single(results, result => result is ContractNameAuthorityConflictException);
        var versions = await firstStore.ListAsync(firstContract.Name, 10, cursor: null);
        var persisted = Assert.Single(versions.Versions);
        Assert.Contains(
            persisted.Contract.Authority,
            new[] { firstContract.Authority, secondContract.Authority });
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

        await store.SetCurrentAsync(versions[1].Contract.Name, versions[1].ContractDigest);
        var current = await store.GetCurrentAsync(versions[1].Contract.Name);

        Assert.NotNull(current);
        Assert.Equal(versions[1].ContractDigest, current.ContractDigest);

        var first = await store.ListAsync(versions[0].Contract.Name, 2, cursor: null);
        Assert.Equal(2, first.Versions.Count);
        Assert.NotNull(first.NextCursor);
        var second = await store.ListAsync(
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
            () => store.ListAsync(versions[0].Contract.Name, 2, "not-a-cursor"));
        await Assert.ThrowsAsync<ContractVersionNotFoundException>(
            () => store.SetCurrentAsync(
                versions[0].Contract.Name,
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"));
    }

    [Fact]
    public async Task ExecutableCandidatesAreImmutableAndPersistent()
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
            .GetAsync(candidate.ExecutableDigest);

        Assert.NotNull(persisted);
        Assert.Equal(ExecutableLifecycleState.Candidate, persisted.State);
        Assert.Equal(0, persisted.StateVersion);
        Assert.Equal(createdAt, persisted.CreatedAt);
        Assert.Equal("test", persisted.Provenance!.Value.GetProperty("source").GetString());
        await Assert.ThrowsAsync<ArgumentException>(() => store.PutCandidateAsync(
            candidate with
            {
                ExecutableDigest =
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            }));
    }

    [Fact]
    public async Task AnalysisCandidateAdmissionIsDurableAndIdempotentPerCycle()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteExecutableStore(database.ConnectionString);
        await store.InitializeAsync();
        var contract = CreateContract(100);
        var candidate = CreateCandidate(contract, variant: 1);
        var admission = CreateAdmission(candidate, cycleId: "cycle-1");

        var created = await store.PutAnalysisCandidateAsync(candidate, admission);
        var retry = await new SqliteExecutableStore(database.ConnectionString)
            .PutAnalysisCandidateAsync(
                candidate with { CreatedAt = candidate.CreatedAt.AddHours(1) },
                admission with { CreatedAt = admission.CreatedAt.AddHours(1) });

        Assert.True(created.Created);
        Assert.False(retry.Created);
        Assert.Equal(created.ExecutableDigest, retry.ExecutableDigest);
        Assert.Equal(admission.CreatedAt, retry.CreatedAt);
        Assert.Equal(
            admission,
            await ReadAnalysisAdmissionAsync(
                database.ConnectionString,
                admission.WorkspaceId,
                admission.CycleId));

        await store.ActivateAsync(
            candidate.Executable.ContractDigest,
            candidate.ExecutableDigest);
        var resolvedRetry = await new SqliteExecutableStore(database.ConnectionString)
            .PutAnalysisCandidateAsync(candidate, admission);
        Assert.False(resolvedRetry.Created);
        Assert.Equal(ExecutableLifecycleState.Active, resolvedRetry.LifecycleState);
        Assert.Equal(created.ExecutableDigest, resolvedRetry.ExecutableDigest);
        Assert.Equal(created.CreatedAt, resolvedRetry.CreatedAt);

        var conflictingCandidate = CreateCandidate(contract, variant: 2);
        var conflict = CreateAdmission(
            conflictingCandidate,
            admission.CycleId,
            admission.WorkspaceId);
        await Assert.ThrowsAsync<CandidateAdmissionConflictException>(
            () => store.PutAnalysisCandidateAsync(conflictingCandidate, conflict));
        Assert.Null(await store.GetAsync(conflictingCandidate.ExecutableDigest));
    }

    [Fact]
    public async Task AnalysisCandidateAdmissionRejectsNonCandidateExecutableDigest()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteExecutableStore(database.ConnectionString);
        await store.InitializeAsync();
        var contract = CreateContract(100);
        var contractDigest = ContractDigests.ComputeContractDigest(contract);
        var previous = CreateCandidate(contract, variant: 1);
        var current = CreateCandidate(contract, variant: 2);
        await store.PutCandidateAsync(previous);
        await store.PutCandidateAsync(current);
        await store.ActivateAsync(contractDigest, previous.ExecutableDigest);

        var activeConflict = await Assert.ThrowsAsync<CandidateLifecycleConflictException>(
            () => store.PutAnalysisCandidateAsync(
                previous,
                CreateAdmission(previous, cycleId: "active-cycle")));
        Assert.Equal(ExecutableLifecycleState.Active, activeConflict.State);

        await store.ActivateAsync(contractDigest, current.ExecutableDigest);
        var inactiveConflict = await Assert.ThrowsAsync<CandidateLifecycleConflictException>(
            () => store.PutAnalysisCandidateAsync(
                previous,
                CreateAdmission(previous, cycleId: "inactive-cycle")));
        Assert.Equal(ExecutableLifecycleState.Inactive, inactiveConflict.State);
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
            firstContractDigest,
            first.ExecutableDigest);
        var unchanged = await store.ActivateAsync(
            firstContractDigest,
            first.ExecutableDigest,
            first.ExecutableDigest);

        Assert.True(initial.Changed);
        Assert.Equal(1, initial.StateVersion);
        Assert.False(unchanged.Changed);
        Assert.Equal(1, unchanged.StateVersion);

        await Assert.ThrowsAsync<ActivationConflictException>(
            () => store.ActivateIfNoneAsync(
                firstContractDigest,
                replacement.ExecutableDigest));
        Assert.Equal(
            first.ExecutableDigest,
            (await store.GetActiveAsync(firstContractDigest))!.ExecutableDigest);

        await Assert.ThrowsAsync<ActivationConflictException>(() => store.ActivateAsync(
            firstContractDigest,
            replacement.ExecutableDigest,
            expectedActiveExecutableDigest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"));
        Assert.Equal(
            first.ExecutableDigest,
            (await store.GetActiveAsync(firstContractDigest))!.ExecutableDigest);

        var replaced = await store.ActivateAsync(
            firstContractDigest,
            replacement.ExecutableDigest,
            first.ExecutableDigest);
        Assert.True(replaced.Changed);
        Assert.Equal(first.ExecutableDigest, replaced.PreviousExecutableDigest);
        Assert.Equal(2, replaced.StateVersion);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await store.GetAsync(first.ExecutableDigest))!.State);
        Assert.Equal(
            ExecutableLifecycleState.Active,
            (await store.GetAsync(replacement.ExecutableDigest))!.State);

        await store.ActivateAsync(secondContractDigest, otherContract.ExecutableDigest);
        Assert.Equal(
            replacement.ExecutableDigest,
            (await store.GetActiveAsync(firstContractDigest))!.ExecutableDigest);
        Assert.Equal(
            otherContract.ExecutableDigest,
            (await store.GetActiveAsync(secondContractDigest))!.ExecutableDigest);

        var reactivated = await store.ActivateAsync(
            firstContractDigest,
            first.ExecutableDigest,
            replacement.ExecutableDigest);
        Assert.Equal(3, reactivated.StateVersion);
        Assert.Equal(
            3,
            (await store.GetAsync(replacement.ExecutableDigest))!.StateVersion);
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
        await setupStore.ActivateAsync(contractDigest, initial.ExecutableDigest);

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
            (await setupStore.GetActiveAsync(contractDigest))!.ExecutableDigest);
    }

    [Fact]
    public async Task AnalysisActivationSelectsNewestCurrentCandidateAndSupersedesBacklog()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var acceptedAt = new DateTimeOffset(2026, 3, 4, 10, 0, 0, TimeSpan.Zero);
        var previousContract = CreateAcceptedVersion(100, acceptedAt);
        var currentContract = CreateAcceptedVersion(200, acceptedAt.AddMinutes(1));
        await contractStore.PutAsync(previousContract);
        await contractStore.PutAsync(currentContract);
        await contractStore.SetCurrentAsync(
            currentContract.Contract.Name,
            currentContract.ContractDigest);

        var initial = CreateCandidate(
            currentContract.Contract,
            variant: 1,
            createdAt: acceptedAt.AddMinutes(2));
        var older = CreateCandidate(
            currentContract.Contract,
            variant: 2,
            createdAt: acceptedAt.AddMinutes(3));
        var newest = CreateCandidate(
            currentContract.Contract,
            variant: 3,
            createdAt: acceptedAt.AddMinutes(4));
        var staleButLater = CreateCandidate(
            previousContract.Contract,
            variant: 4,
            createdAt: acceptedAt.AddMinutes(5));
        await executableStore.PutCandidateAsync(initial);
        await executableStore.ActivateAsync(
            currentContract.ContractDigest,
            initial.ExecutableDigest);
        await executableStore.PutAnalysisCandidateAsync(
            older,
            CreateAdmission(older, "cycle-older"));
        await executableStore.PutAnalysisCandidateAsync(
            newest,
            CreateAdmission(newest, "cycle-newest"));
        await executableStore.PutAnalysisCandidateAsync(
            staleButLater,
            CreateAdmission(staleButLater, "cycle-stale"));

        var snapshot = Assert.Single(
            (await executableStore.ListPendingAsync(10)).Candidates);

        Assert.Equal(newest.ExecutableDigest, snapshot.Admission.ExecutableDigest);
        Assert.Equal(initial.ExecutableDigest, snapshot.ActiveExecutable?.ExecutableDigest);

        var result = await executableStore.ResolveAsync(
            new AnalysisCandidateResolutionCommand(
                snapshot.Admission,
                snapshot.CandidateStateVersion,
                snapshot.ActiveExecutable,
                AnalysisCandidateResolutionAction.Activate));

        Assert.Equal(AnalysisCandidateResolutionOutcome.Activated, result.Outcome);
        Assert.Equal(2, result.SupersededCandidateCount);
        Assert.Equal(
            newest.ExecutableDigest,
            (await executableStore.GetActiveAsync(currentContract.ContractDigest))!
                .ExecutableDigest);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await executableStore.GetAsync(older.ExecutableDigest))!.State);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await executableStore.GetAsync(staleButLater.ExecutableDigest))!.State);
        Assert.Empty((await executableStore.ListPendingAsync(10)).Candidates);
    }

    [Fact]
    public async Task AnalysisActivationSupersedesCandidateAfterCurrentContractMoves()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var acceptedAt = new DateTimeOffset(2026, 3, 4, 11, 0, 0, TimeSpan.Zero);
        var previousContract = CreateAcceptedVersion(100, acceptedAt);
        var currentContract = CreateAcceptedVersion(200, acceptedAt.AddMinutes(1));
        await contractStore.PutAsync(previousContract);
        await contractStore.PutAsync(currentContract);
        await contractStore.SetCurrentAsync(
            previousContract.Contract.Name,
            previousContract.ContractDigest);
        var initial = CreateCandidate(previousContract.Contract, variant: 1);
        var candidate = CreateCandidate(previousContract.Contract, variant: 2);
        await executableStore.PutCandidateAsync(initial);
        await executableStore.ActivateAsync(
            previousContract.ContractDigest,
            initial.ExecutableDigest);
        await executableStore.PutAnalysisCandidateAsync(
            candidate,
            CreateAdmission(candidate, "cycle-stale"));
        var snapshot = Assert.Single(
            (await executableStore.ListPendingAsync(10)).Candidates);

        await contractStore.SetCurrentAsync(
            currentContract.Contract.Name,
            currentContract.ContractDigest);
        var result = await executableStore.ResolveAsync(
            new AnalysisCandidateResolutionCommand(
                snapshot.Admission,
                snapshot.CandidateStateVersion,
                snapshot.ActiveExecutable,
                AnalysisCandidateResolutionAction.Activate));

        Assert.Equal(AnalysisCandidateResolutionOutcome.Superseded, result.Outcome);
        Assert.Equal(
            ExecutableLifecycleState.Inactive,
            (await executableStore.GetAsync(candidate.ExecutableDigest))!.State);
        Assert.Equal(
            initial.ExecutableDigest,
            (await executableStore.GetActiveAsync(previousContract.ContractDigest))!
                .ExecutableDigest);
    }

    [Fact]
    public async Task AnalysisActivationDetectsActiveExecutableAbaByStateVersion()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var accepted = CreateAcceptedVersion(
            100,
            new DateTimeOffset(2026, 3, 4, 12, 0, 0, TimeSpan.Zero));
        await contractStore.PutAsync(accepted);
        await contractStore.SetCurrentAsync(
            accepted.Contract.Name,
            accepted.ContractDigest);
        var initial = CreateCandidate(accepted.Contract, variant: 1);
        var intervening = CreateCandidate(accepted.Contract, variant: 2);
        var candidate = CreateCandidate(accepted.Contract, variant: 3);
        await executableStore.PutCandidateAsync(initial);
        await executableStore.PutCandidateAsync(intervening);
        await executableStore.ActivateAsync(
            accepted.ContractDigest,
            initial.ExecutableDigest);
        await executableStore.PutAnalysisCandidateAsync(
            candidate,
            CreateAdmission(candidate, "cycle-aba"));
        var snapshot = Assert.Single(
            (await executableStore.ListPendingAsync(10)).Candidates);

        await executableStore.ActivateAsync(
            accepted.ContractDigest,
            intervening.ExecutableDigest,
            initial.ExecutableDigest);
        await executableStore.ActivateAsync(
            accepted.ContractDigest,
            initial.ExecutableDigest,
            intervening.ExecutableDigest);
        var result = await executableStore.ResolveAsync(
            new AnalysisCandidateResolutionCommand(
                snapshot.Admission,
                snapshot.CandidateStateVersion,
                snapshot.ActiveExecutable,
                AnalysisCandidateResolutionAction.Activate));

        Assert.Equal(AnalysisCandidateResolutionOutcome.Yielded, result.Outcome);
        Assert.Equal(
            ExecutableLifecycleState.Candidate,
            (await executableStore.GetAsync(candidate.ExecutableDigest))!.State);
        Assert.Equal(
            initial.ExecutableDigest,
            (await executableStore.GetActiveAsync(accepted.ContractDigest))!
                .ExecutableDigest);
    }

    [Fact]
    public async Task ConcurrentAnalysisActivationHasOneWinnerAndOneIdempotentYield()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var setupStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await setupStore.InitializeAsync();
        var accepted = CreateAcceptedVersion(
            100,
            new DateTimeOffset(2026, 3, 4, 13, 0, 0, TimeSpan.Zero));
        await contractStore.PutAsync(accepted);
        await contractStore.SetCurrentAsync(
            accepted.Contract.Name,
            accepted.ContractDigest);
        var initial = CreateCandidate(accepted.Contract, variant: 1);
        var candidate = CreateCandidate(accepted.Contract, variant: 2);
        await setupStore.PutCandidateAsync(initial);
        await setupStore.ActivateAsync(
            accepted.ContractDigest,
            initial.ExecutableDigest);
        await setupStore.PutAnalysisCandidateAsync(
            candidate,
            CreateAdmission(candidate, "cycle-concurrent"));
        var snapshot = Assert.Single(
            (await setupStore.ListPendingAsync(10)).Candidates);
        var command = new AnalysisCandidateResolutionCommand(
            snapshot.Admission,
            snapshot.CandidateStateVersion,
            snapshot.ActiveExecutable,
            AnalysisCandidateResolutionAction.Activate);

        var results = await Task.WhenAll(
            new SqliteExecutableStore(database.ConnectionString).ResolveAsync(command),
            new SqliteExecutableStore(database.ConnectionString).ResolveAsync(command));

        Assert.Single(
            results,
            result => result.Outcome == AnalysisCandidateResolutionOutcome.Activated);
        Assert.Single(
            results,
            result => result.Outcome
                == AnalysisCandidateResolutionOutcome.AlreadyResolved);
        Assert.Equal(
            candidate.ExecutableDigest,
            (await setupStore.GetActiveAsync(accepted.ContractDigest))!
                .ExecutableDigest);
    }

    [Fact]
    public async Task FailedAnalysisActivationRollsBackAndLeavesCandidatePending()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var accepted = CreateAcceptedVersion(
            100,
            new DateTimeOffset(2026, 3, 4, 14, 0, 0, TimeSpan.Zero));
        await contractStore.PutAsync(accepted);
        await contractStore.SetCurrentAsync(
            accepted.Contract.Name,
            accepted.ContractDigest);
        var initial = CreateCandidate(accepted.Contract, variant: 1);
        var candidate = CreateCandidate(accepted.Contract, variant: 2);
        await executableStore.PutCandidateAsync(initial);
        await executableStore.ActivateAsync(
            accepted.ContractDigest,
            initial.ExecutableDigest);
        await executableStore.PutAnalysisCandidateAsync(
            candidate,
            CreateAdmission(candidate, "cycle-failure"));
        var snapshot = Assert.Single(
            (await executableStore.ListPendingAsync(10)).Candidates);
        await ExecuteSqlAsync(
            database.ConnectionString,
            $"""
            CREATE TRIGGER fail_analysis_activation
            BEFORE UPDATE OF lifecycle_state ON decision_executables
            WHEN NEW.executable_digest = '{candidate.ExecutableDigest}'
             AND NEW.lifecycle_state = 'active'
            BEGIN
                SELECT RAISE(ABORT, 'injected activation failure');
            END;
            """);

        await Assert.ThrowsAsync<SqliteException>(() => executableStore.ResolveAsync(
            new AnalysisCandidateResolutionCommand(
                snapshot.Admission,
                snapshot.CandidateStateVersion,
                snapshot.ActiveExecutable,
                AnalysisCandidateResolutionAction.Activate)));

        Assert.Equal(
            initial.ExecutableDigest,
            (await executableStore.GetActiveAsync(accepted.ContractDigest))!
                .ExecutableDigest);
        Assert.Equal(
            ExecutableLifecycleState.Candidate,
            (await executableStore.GetAsync(candidate.ExecutableDigest))!.State);
    }

    [Fact]
    public async Task AnalysisCandidateDiscoveryPagesByContractWithoutStarvation()
    {
        using var database = new TemporaryDatabase();
        var contractStore = new SqliteContractVersionStore(database.ConnectionString);
        var executableStore = new SqliteExecutableStore(database.ConnectionString);
        await contractStore.InitializeAsync();
        await executableStore.InitializeAsync();
        var firstContract = CreateContract(100);
        var secondContract = CreateContract(200) with { Name = "inventory.limit" };
        var acceptedAt = new DateTimeOffset(2026, 3, 4, 15, 0, 0, TimeSpan.Zero);
        foreach (var contract in new[] { firstContract, secondContract })
        {
            var version = new AcceptedContractVersion(
                ContractDigests.ComputeContractDigest(contract),
                acceptedAt,
                contract);
            await contractStore.PutAsync(version);
            await contractStore.SetCurrentAsync(
                contract.Name,
                version.ContractDigest);
        }

        var firstCandidate = CreateCandidate(firstContract, variant: 1);
        var secondCandidate = CreateCandidate(secondContract, variant: 2);
        await executableStore.PutAnalysisCandidateAsync(
            firstCandidate,
            CreateAdmission(
                firstCandidate,
                "cycle-first",
                contractName: firstContract.Name));
        await executableStore.PutAnalysisCandidateAsync(
            secondCandidate,
            CreateAdmission(
                secondCandidate,
                "cycle-second",
                contractName: secondContract.Name));

        var firstPage = await executableStore.ListPendingAsync(1);
        var first = Assert.Single(firstPage.Candidates);
        Assert.NotNull(firstPage.NextContractName);
        var secondPage = await executableStore.ListPendingAsync(
            1,
            firstPage.NextContractName);
        var second = Assert.Single(secondPage.Candidates);

        Assert.NotEqual(
            first.Admission.ContractName,
            second.Admission.ContractName);
        Assert.Null(secondPage.NextContractName);
    }

    [Fact]
    public async Task AnalysisCandidateDiscoveryStartsFromPendingExecutables()
    {
        using var database = new TemporaryDatabase();
        var store = new SqliteExecutableStore(database.ConnectionString);
        await store.InitializeAsync();
        await using var connection = new SqliteConnection(database.ConnectionString);
        await connection.OpenAsync();
        await using var command = connection.CreateCommand();
        command.CommandText =
            """
            EXPLAIN QUERY PLAN
            SELECT admission.contract_name
            FROM decision_executables AS executable
            CROSS JOIN analysis_candidate_admissions AS admission
            WHERE executable.lifecycle_state = 'candidate'
              AND admission.executable_digest = executable.executable_digest;
            """;
        var plan = new List<string>();
        await using var reader = await command.ExecuteReaderAsync();
        while (await reader.ReadAsync())
        {
            plan.Add(reader.GetString(3));
        }

        Assert.Contains(
            plan,
            step => step.Contains(
                "ix_executables_analysis_candidates",
                StringComparison.Ordinal));
        Assert.Contains(
            plan,
            step => step.Contains(
                "ix_analysis_candidates_executable",
                StringComparison.Ordinal));
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

    [Theory]
    [InlineData("contract-store")]
    [InlineData("executable-store")]
    public async Task InitializationRejectsVersionOneSchemasWithoutMigration(string component)
    {
        using var database = new TemporaryDatabase();
        await ExecuteSqlAsync(
            database.ConnectionString,
            "CREATE TABLE flaggo_schema_versions ("
            + "component TEXT PRIMARY KEY NOT NULL, "
            + "version INTEGER NOT NULL); "
            + $"INSERT INTO flaggo_schema_versions(component, version) "
            + $"VALUES ('{component}', 1);");

        Task InitializeAsync() => component switch
        {
            "contract-store" => new SqliteContractVersionStore(database.ConnectionString)
                .InitializeAsync(),
            "executable-store" => new SqliteExecutableStore(database.ConnectionString)
                .InitializeAsync(),
            _ => throw new InvalidOperationException($"Unknown component '{component}'.")
        };

        var exception = await Assert.ThrowsAsync<InvalidOperationException>(InitializeAsync);
        Assert.Equal(
            $"Unsupported {component} schema version 1; "
            + $"expected {(component == "executable-store" ? 4 : 3)}.",
            exception.Message);
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
            "UPDATE flaggo_schema_versions SET version = 4 "
            + "WHERE component = 'contract-store';");
        Assert.False(await contractStore.IsAvailableAsync());
        Assert.True(await executableStore.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "UPDATE flaggo_schema_versions SET version = 3 "
            + "WHERE component = 'contract-store'; "
            + "UPDATE flaggo_schema_versions SET version = 5 "
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

        await contractStore.InitializeAsync();
        Assert.True(await contractStore.IsAvailableAsync());

        await ExecuteSqlAsync(
            database.ConnectionString,
            "DROP TABLE analysis_candidate_admissions;");
        Assert.True(await contractStore.IsAvailableAsync());
        Assert.False(await executableStore.IsAvailableAsync());

        await executableStore.InitializeAsync();
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
            ContractDigests.ComputeContractDigest(contract),
            acceptedAt,
            contract);
    }

    private static DecisionContract CreateContract(int defaultValue) =>
        new()
        {
            Authority = Scope,
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

    private static AnalysisCandidateAdmission CreateAdmission(
        StoredExecutable candidate,
        string cycleId,
        string? workspaceId = null,
        string contractName = "checkout.delay") =>
        new(
            workspaceId
                ?? "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            cycleId,
            $"attempt-{cycleId}",
            contractName,
            candidate.Executable.ContractDigest,
            candidate.ExecutableDigest,
            candidate.CreatedAt.AddMinutes(-5),
            42,
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            candidate.CreatedAt);

    private static async Task<AnalysisCandidateAdmission?> ReadAnalysisAdmissionAsync(
        string connectionString,
        string workspaceId,
        string cycleId)
    {
        await using var connection = new SqliteConnection(connectionString);
        await connection.OpenAsync();
        await using var command = connection.CreateCommand();
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
        await using var reader = await command.ExecuteReaderAsync();
        return await reader.ReadAsync()
            ? new AnalysisCandidateAdmission(
                workspaceId,
                cycleId,
                reader.GetString(0),
                reader.GetString(1),
                reader.GetString(2),
                reader.GetString(3),
                DateTimeOffset.Parse(
                    reader.GetString(4),
                    System.Globalization.CultureInfo.InvariantCulture),
                reader.GetInt64(5),
                reader.GetString(6),
                DateTimeOffset.Parse(
                    reader.GetString(7),
                    System.Globalization.CultureInfo.InvariantCulture))
            : null;
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
