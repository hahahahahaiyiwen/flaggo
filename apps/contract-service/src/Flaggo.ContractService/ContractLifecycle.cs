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

    Task<AnalysisCandidateResult> SubmitAnalysisCandidateAsync(
        string contractName,
        string contractDigest,
        AnalysisCandidateSubmission submission,
        CancellationToken cancellationToken = default);
}

public sealed class ContractLifecycle(
    IContractVersionStore contractStore,
    IExecutableStore executableStore,
    FlaggoExpressionCompiler expressionCompiler,
    FlaggoExecutableCompiler executableCompiler,
    TimeProvider timeProvider,
    ContractServiceObservability observability) : IContractLifecycle
{
    private readonly SemaphoreSlim currentMutation = new(1, 1);

    public DecisionContractValidationResult Validate(
        string contractName,
        DecisionContract contract)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.validate",
            "contract.validate",
            contractName);
        try
        {
            ValidateRouteName(contractName);
            if (!string.Equals(contractName, contract.Name, StringComparison.Ordinal))
            {
                throw new ContractNameMismatchException(contractName, contract.Name);
            }

            var validation = ContractValidator.Validate(contract, expressionCompiler);
            operation.Activity?.SetTag(
                "flaggo.contract.digest",
                validation.ContractDigest);
            operation.Complete(validation.Status == "valid" ? "success" : "invalid");
            return validation;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<ContractDeploymentResult> DeployAsync(
        string contractName,
        DecisionContract contract,
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.deploy",
            "contract.deploy",
            contractName);
        try
        {
            var validation = Validate(contractName, contract);
            if (validation.Status != "valid" || validation.ContractDigest is null)
            {
                throw new InvalidDecisionContractException(validation.Issues);
            }

            var contractDigest = validation.ContractDigest;
            operation.Activity?.SetTag("flaggo.contract.digest", contractDigest);
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
            await currentMutation.WaitAsync(cancellationToken);
            try
            {
                await contractStore.SetCurrentAsync(
                    contractName,
                    contractDigest,
                    cancellationToken);
            }
            finally
            {
                currentMutation.Release();
            }

            operation.Activity?.SetTag(
                "flaggo.executable.digest",
                active.ExecutableDigest);
            operation.Complete("success");

            return new ContractDeploymentResult(
                Project(accepted, active.ExecutableDigest),
                writeResult == ContractStoreWriteResult.Created);
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<DecisionContractVersion?> GetCurrentAsync(
        string contractName,
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.read",
            "contract.read",
            contractName);
        try
        {
            ValidateRouteName(contractName);
            var accepted = await contractStore.GetCurrentAsync(
                contractName,
                cancellationToken);
            var result = accepted is null
                ? null
                : await ProjectReadyAsync(accepted, cancellationToken);
            operation.Activity?.SetTag(
                "flaggo.contract.digest",
                result?.ContractDigest);
            operation.Complete(result is null ? "not_found" : "success");
            return result;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<CurrentContractCatalogResult> GetCurrentCatalogAsync(
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.catalog.read",
            "contract.catalog.read");
        try
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

            var result = CurrentContractCatalogs.Project(current);
            operation.Complete("success");
            return result;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<DecisionContractVersion?> GetAsync(
        string contractName,
        string contractDigest,
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.read",
            "contract.read",
            contractName,
            contractDigest);
        try
        {
            ValidateRouteName(contractName);
            ValidateDigest(contractDigest);
            var accepted = await contractStore.GetAsync(
                contractName,
                contractDigest,
                cancellationToken);
            var result = accepted is null
                ? null
                : await ProjectReadyAsync(accepted, cancellationToken);
            operation.Complete(result is null ? "not_found" : "success");
            return result;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<DecisionContractVersionList?> ListAsync(
        string contractName,
        int pageSize,
        string? cursor,
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartContractOperation(
            "flaggo.contract.read",
            "contract.read",
            contractName);
        try
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
                operation.Complete("not_found");
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

            var result = new DecisionContractVersionList
            {
                Name = contractName,
                CurrentContractDigest = current.ContractDigest,
                Versions = versions,
                NextCursor = page.NextCursor
            };
            operation.Complete("success");
            return result;
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            throw;
        }
    }

    public async Task<AnalysisCandidateResult> SubmitAnalysisCandidateAsync(
        string contractName,
        string contractDigest,
        AnalysisCandidateSubmission submission,
        CancellationToken cancellationToken = default)
    {
        using var operation = observability.StartCandidateSubmission(
            contractName,
            contractDigest);
        string? candidateDigest = null;
        try
        {
            ValidateRouteName(contractName);
            ValidateDigest(contractDigest);
            ArgumentNullException.ThrowIfNull(submission);
            ValidateProvenance(submission.Provenance, timeProvider.GetUtcNow());
            var accepted = await contractStore.GetAsync(
                contractName,
                contractDigest,
                cancellationToken);
            if (accepted is null)
            {
                throw new ContractVersionNotFoundException(contractName, contractDigest);
            }
            var compilation = executableCompiler.Compile(
                accepted.Contract,
                new DecisionExecutable
                {
                    ContractDigest = contractDigest,
                    Rules = submission.Rules
                });
            candidateDigest = compilation.ExecutableDigest;
            operation.Activity?.SetTag(
                "flaggo.candidate.digest",
                candidateDigest);
            var createdAt = timeProvider.GetUtcNow();
            var stored = new StoredExecutable(
                candidateDigest,
                compilation.Executable,
                compilation.CheckedExecutable,
                ExecutableLifecycleState.Candidate,
                0,
                createdAt,
                null,
                JsonSerializer.SerializeToElement(new
                {
                    source = "async-analysis",
                    workspaceId = submission.Provenance.WorkspaceId,
                    cycleId = submission.Provenance.CycleId,
                    attemptId = submission.Provenance.AttemptId,
                    evidenceCutoff = submission.Provenance.EvidenceCutoff,
                    evidenceWatermark = submission.Provenance.EvidenceWatermark,
                    analysisManifestDigest = submission.Provenance.AnalysisManifestDigest
                }));
            var admission = new AnalysisCandidateAdmission(
                submission.Provenance.WorkspaceId,
                submission.Provenance.CycleId,
                submission.Provenance.AttemptId,
                contractName,
                contractDigest,
                candidateDigest,
                submission.Provenance.EvidenceCutoff,
                submission.Provenance.EvidenceWatermark,
                submission.Provenance.AnalysisManifestDigest,
                createdAt);

            await currentMutation.WaitAsync(cancellationToken);
            try
            {
                var current = await contractStore.GetCurrentAsync(
                    contractName,
                    cancellationToken);
                if (current is null
                    || !string.Equals(
                        current.ContractDigest,
                        contractDigest,
                        StringComparison.Ordinal))
                {
                    throw new StaleContractDigestException(
                        contractName,
                        contractDigest,
                        current?.ContractDigest);
                }

                var result = await executableStore.PutAnalysisCandidateAsync(
                    stored,
                    admission,
                    cancellationToken);
                operation.Complete("success");
                observability.CandidateAdmitted(
                    contractName,
                    contractDigest,
                    candidateDigest);
                return new AnalysisCandidateResult
                {
                    ContractName = contractName,
                    ContractDigest = contractDigest,
                    ExecutableDigest = result.ExecutableDigest,
                    LifecycleState = result.LifecycleState switch
                    {
                        ExecutableLifecycleState.Candidate => "candidate",
                        ExecutableLifecycleState.Active => "active",
                        ExecutableLifecycleState.Inactive => "inactive",
                        _ => throw new InvalidDataException(
                            $"Unknown executable lifecycle state "
                            + $"'{result.LifecycleState}'.")
                    },
                    CreatedAt = result.CreatedAt,
                    Created = result.Created
                };
            }
            finally
            {
                currentMutation.Release();
            }
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            observability.CandidateRejected(
                contractName,
                contractDigest,
                candidateDigest,
                exception);
            throw;
        }
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
                accepted.Contract.Name,
                accepted.ContractDigest,
                defaultCompilation,
                "default",
                cancellationToken);
            using var activation = observability.StartActivation(
                accepted.Contract.Name,
                accepted.ContractDigest,
                defaultCompilation.ExecutableDigest);
            try
            {
                await executableStore.ActivateIfNoneAsync(
                    accepted.ContractDigest,
                    defaultCompilation.ExecutableDigest,
                    cancellationToken);
                activation.Complete("success");
                observability.ActivationCompleted(
                    accepted.Contract.Name,
                    accepted.ContractDigest,
                    defaultCompilation.ExecutableDigest);
            }
            catch (ActivationConflictException exception)
            {
                activation.Fail(exception, "conflict", "conflict");
                observability.ActivationRejected(
                    accepted.Contract.Name,
                    accepted.ContractDigest,
                    defaultCompilation.ExecutableDigest,
                    exception);
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
            accepted.Contract.Name,
            accepted.ContractDigest,
            authoredCompilation,
            "authored",
            cancellationToken);
        using var authoredActivation = observability.StartActivation(
            accepted.Contract.Name,
            accepted.ContractDigest,
            authoredCompilation.ExecutableDigest);
        try
        {
            await executableStore.ActivateAsync(
                accepted.ContractDigest,
                authoredCompilation.ExecutableDigest,
                defaultCompilation.ExecutableDigest,
                cancellationToken);
            authoredActivation.Complete("success");
            observability.ActivationCompleted(
                accepted.Contract.Name,
                accepted.ContractDigest,
                authoredCompilation.ExecutableDigest);
        }
        catch (ActivationConflictException exception)
        {
            authoredActivation.Fail(exception, "conflict", "conflict");
            observability.ActivationRejected(
                accepted.Contract.Name,
                accepted.ContractDigest,
                authoredCompilation.ExecutableDigest,
                exception);
        }

        return await executableStore.GetActiveAsync(
                accepted.ContractDigest,
                cancellationToken)
            ?? throw new InvalidOperationException(
                "Authored executable activation removed runtime authority.");
    }

    private async Task PutCandidateAsync(
        string contractName,
        string contractDigest,
        ExecutableCompilation compilation,
        string source,
        CancellationToken cancellationToken)
    {
        using var operation = observability.StartCandidateSubmission(
            contractName,
            contractDigest,
            compilation.ExecutableDigest);
        try
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
            operation.Complete("success");
            observability.CandidateAdmitted(
                contractName,
                contractDigest,
                compilation.ExecutableDigest);
        }
        catch (Exception exception)
        {
            var (outcome, category) = ContractServiceObservability.Classify(exception);
            operation.Fail(exception, outcome, category);
            observability.CandidateRejected(
                contractName,
                contractDigest,
                compilation.ExecutableDigest,
                exception);
            throw;
        }
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

    private static void ValidateProvenance(
        AnalysisCandidateProvenance provenance,
        DateTimeOffset now)
    {
        ArgumentNullException.ThrowIfNull(provenance);
        foreach (var (name, value) in new[]
                 {
                     ("workspaceId", provenance.WorkspaceId),
                     ("cycleId", provenance.CycleId),
                     ("attemptId", provenance.AttemptId)
                 })
        {
            if (string.IsNullOrWhiteSpace(value)
                || value.Length > 256
                || value.Any(char.IsControl))
            {
                throw new ArgumentException(
                    $"{name} must contain 1-256 non-control characters.");
            }
        }

        if (!ContractDigests.IsSha256Digest(provenance.WorkspaceId)
            || !ContractDigests.IsSha256Digest(provenance.AnalysisManifestDigest))
        {
            throw new ArgumentException(
                "workspaceId and analysisManifestDigest must be lowercase sha256 digests.");
        }

        if (provenance.EvidenceWatermark <= 0)
        {
            throw new ArgumentException("evidenceWatermark must be positive.");
        }

        if (provenance.EvidenceCutoff > now)
        {
            throw new ArgumentException("evidenceCutoff cannot be in the future.");
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

public sealed class StaleContractDigestException(
    string contractName,
    string requestedDigest,
    string? currentDigest)
    : Exception(
        $"Contract '{contractName}' current digest is '{currentDigest ?? "<none>"}', "
        + $"not '{requestedDigest}'.");
