using System.Text.Json;
using Flaggo.Contract;
using Flaggo.ContractStore;
using Flaggo.ExecutableStore;
using Flaggo.Expressions;

namespace Flaggo.ContractService;

public interface IContractLifecycle
{
    DecisionContractValidationResult Validate(
        string contractName,
        DecisionContract contract);

    Task<ContractDeploymentResult> DeployAsync(
        string contractName,
        DecisionContract contract,
        CancellationToken cancellationToken = default);

    Task<DecisionContractVersion?> GetCurrentAsync(
        string contractName,
        CancellationToken cancellationToken = default);

    Task<CurrentContractCatalogResult> GetCurrentCatalogAsync(
        CancellationToken cancellationToken = default);

    Task<DecisionContractVersion?> GetAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default);

    Task<DecisionContractVersionList?> ListAsync(
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default);
}

public sealed class ContractLifecycle(
    IContractVersionStore contractStore,
    IExecutableStore executableStore,
    FlaggoExpressionCompiler expressionCompiler,
    FlaggoExecutableCompiler executableCompiler,
    TimeProvider timeProvider) : IContractLifecycle
{
    public DecisionContractValidationResult Validate(
        string contractName,
        DecisionContract contract)
    {
        ValidateRouteName(contractName);
        if (!string.Equals(contractName, contract.Name, StringComparison.Ordinal))
        {
            throw new ContractNameMismatchException(contractName, contract.Name);
        }

        return ContractValidator.Validate(contract, expressionCompiler);
    }

    public async Task<ContractDeploymentResult> DeployAsync(
        string contractName,
        DecisionContract contract,
        CancellationToken cancellationToken = default)
    {
        var validation = Validate(contractName, contract);
        if (validation.Status != "valid" || validation.ContractDigest is null)
        {
            throw new InvalidDecisionContractException(validation.Issues);
        }

        var contractDigest = validation.ContractDigest;
        var acceptedAt = timeProvider.GetUtcNow();
        var pendingVersion = new AcceptedContractVersion(
            contractDigest,
            acceptedAt,
            contract);
        var defaultCompilation = CompileDefault(pendingVersion);
        var authoredCompilation = CompileAuthored(pendingVersion);
        var writeResult = await contractStore.PutAsync(
            pendingVersion,
            cancellationToken);
        var accepted = await contractStore.GetAsync(
                contractName,
                contractDigest,
                cancellationToken)
            ?? throw new InvalidOperationException(
                "The accepted contract could not be read after persistence.");

        var active = await EnsureRuntimeReadyAsync(
            accepted,
            defaultCompilation,
            authoredCompilation,
            cancellationToken);
        await contractStore.SetCurrentAsync(
            contractName,
            contractDigest,
            cancellationToken);

        return new ContractDeploymentResult(
            Project(accepted, active.ExecutableDigest),
            writeResult == ContractStoreWriteResult.Created);
    }

    public async Task<DecisionContractVersion?> GetCurrentAsync(
        string contractName,
        CancellationToken cancellationToken = default)
    {
        ValidateRouteName(contractName);
        var accepted = await contractStore.GetCurrentAsync(
            contractName,
            cancellationToken);
        return accepted is null
            ? null
            : await ProjectReadyAsync(accepted, cancellationToken);
    }

    public async Task<CurrentContractCatalogResult> GetCurrentCatalogAsync(
        CancellationToken cancellationToken = default)
    {
        var current = await contractStore.ListAllCurrentAsync(cancellationToken);
        foreach (var version in current)
        {
            if (await executableStore.GetActiveAsync(
                version.ContractDigest,
                cancellationToken) is null)
            {
                throw new InvalidDataException(
                    $"Current contract '{version.ContractDigest}' has no active executable.");
            }
        }

        return CurrentContractCatalogs.Project(current);
    }

    public async Task<DecisionContractVersion?> GetAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default)
    {
        ValidateRouteName(contractName);
        ValidateDigest(contractDigest);
        var accepted = await contractStore.GetAsync(
            contractName,
            contractDigest,
            cancellationToken);
        return accepted is null
            ? null
            : await ProjectReadyAsync(accepted, cancellationToken);
    }

    public async Task<DecisionContractVersionList?> ListAsync(
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default)
    {
        ValidateRouteName(contractName);
        if (pageSize is < 1 or > 200)
        {
            throw new ArgumentOutOfRangeException(
                nameof(pageSize),
                "Page size must be between 1 and 200.");
        }

        var current = await contractStore.GetCurrentAsync(
            contractName,
            cancellationToken);
        if (current is null)
        {
            return null;
        }

        var page = await contractStore.ListAsync(
            contractName,
            pageSize,
            cursor,
            cancellationToken);
        var versions = new List<DecisionContractVersionSummary>(page.Versions.Count);
        foreach (var accepted in page.Versions)
        {
            var active = await executableStore.GetActiveAsync(
                accepted.ContractDigest,
                cancellationToken);
            if (active is null)
            {
                throw new InvalidDataException(
                    $"Deployed contract '{accepted.ContractDigest}' has no active executable.");
            }

            versions.Add(new DecisionContractVersionSummary
            {
                ContractDigest = accepted.ContractDigest,
                AcceptedAt = accepted.AcceptedAt,
                ActiveExecutableDigest = active.ExecutableDigest
            });
        }

        return new DecisionContractVersionList
        {
            Name = contractName,
            CurrentContractDigest = current.ContractDigest,
            Versions = versions,
            NextCursor = page.NextCursor
        };
    }

    private async Task<StoredExecutable> EnsureRuntimeReadyAsync(
        AcceptedContractVersion accepted,
        ExecutableCompilation defaultCompilation,
        ExecutableCompilation? authoredCompilation,
        CancellationToken cancellationToken)
    {
        var active = await executableStore.GetActiveAsync(
            accepted.ContractDigest,
            cancellationToken);
        if (active is null)
        {
            await PutCandidateAsync(
                defaultCompilation,
                "default",
                cancellationToken);
            try
            {
                await executableStore.ActivateIfNoneAsync(
                    accepted.ContractDigest,
                    defaultCompilation.ExecutableDigest,
                    cancellationToken);
            }
            catch (ActivationConflictException)
            {
                // Another deployer or generator established runtime authority.
            }

            active = await executableStore.GetActiveAsync(
                accepted.ContractDigest,
                cancellationToken);
        }

        if (authoredCompilation is null
            || !string.Equals(
                active?.ExecutableDigest,
                defaultCompilation.ExecutableDigest,
                StringComparison.Ordinal))
        {
            return active
                ?? throw new InvalidOperationException(
                    "Default executable activation did not produce runtime authority.");
        }

        await PutCandidateAsync(
            authoredCompilation,
            "authored",
            cancellationToken);
        try
        {
            await executableStore.ActivateAsync(
                accepted.ContractDigest,
                authoredCompilation.ExecutableDigest,
                defaultCompilation.ExecutableDigest,
                cancellationToken);
        }
        catch (ActivationConflictException)
        {
            // A concurrent activation won. Runtime authority remains valid and is
            // projected below rather than being overwritten by generation order.
        }

        return await executableStore.GetActiveAsync(
                accepted.ContractDigest,
                cancellationToken)
            ?? throw new InvalidOperationException(
                "Authored executable activation removed runtime authority.");
    }

    private async Task PutCandidateAsync(
        ExecutableCompilation compilation,
        string source,
        CancellationToken cancellationToken)
    {
        await executableStore.PutCandidateAsync(
            new StoredExecutable(
                compilation.ExecutableDigest,
                compilation.Executable,
                compilation.CheckedExecutable,
                ExecutableLifecycleState.Candidate,
                0,
                timeProvider.GetUtcNow(),
                null,
                JsonSerializer.SerializeToElement(new { source })),
            cancellationToken);
    }

    private ExecutableCompilation CompileDefault(
        AcceptedContractVersion accepted) =>
        executableCompiler.Compile(
            accepted.Contract,
            new DecisionExecutable
            {
                ContractDigest = accepted.ContractDigest,
                Rules = []
            });

    private ExecutableCompilation? CompileAuthored(
        AcceptedContractVersion accepted)
    {
        var authored = accepted.Contract.AuthoredExecutable;
        if (authored is null
            || authored.Rules.Any(rule => rule.When is not ExpressionWhen))
        {
            return null;
        }

        return executableCompiler.Compile(
            accepted.Contract,
            new DecisionExecutable
            {
                ContractDigest = accepted.ContractDigest,
                Rules = authored.Rules.Select(rule => new ExecutableRule
                {
                    Name = rule.Name,
                    When = (ExpressionWhen)rule.When,
                    Return = rule.Return
                }).ToArray()
            });
    }

    private async Task<DecisionContractVersion?> ProjectReadyAsync(
        AcceptedContractVersion accepted,
        CancellationToken cancellationToken)
    {
        var active = await executableStore.GetActiveAsync(
            accepted.ContractDigest,
            cancellationToken);
        return active is null
            ? null
            : Project(accepted, active.ExecutableDigest);
    }

    private static DecisionContractVersion Project(
        AcceptedContractVersion accepted,
        string activeExecutableDigest) =>
        new()
        {
            Name = accepted.Contract.Name,
            ContractDigest = accepted.ContractDigest,
            AcceptedAt = accepted.AcceptedAt,
            ActiveExecutableDigest = activeExecutableDigest,
            Contract = accepted.Contract
        };

    private static void ValidateRouteName(string contractName)
    {
        if (!ContractValidator.IsDecisionName(contractName))
        {
            throw new ArgumentException("The contract name is invalid.", nameof(contractName));
        }
    }

    private static void ValidateDigest(string contractDigest)
    {
        if (!ContractDigests.IsSha256Digest(contractDigest))
        {
            throw new ArgumentException(
                "The contract digest is invalid.",
                nameof(contractDigest));
        }
    }
}

public sealed record ContractDeploymentResult(
    DecisionContractVersion Version,
    bool Created);

public sealed class ContractNameMismatchException(
    string routeName,
    string payloadName)
    : Exception(
        $"Route contract name '{routeName}' does not match payload name '{payloadName}'.");

public sealed class InvalidDecisionContractException(
    IReadOnlyList<ValidationIssue> issues)
    : Exception("The DecisionContract is semantically invalid.")
{
    public IReadOnlyList<ValidationIssue> Issues { get; } = issues;
}
